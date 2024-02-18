use std::collections::HashMap;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use atomic_refcell::AtomicRefCell;

use audio_synchronization::mpsc_counter::MpscCounter;
use crossbeam::channel as chan;
use rayon::prelude::*;
use rayon::ThreadPool;

use super::*;
use crate::unique_id::UniqueId;

/// The worker pool's background threads will wake up this often to find out if the pool has gone away.
const SHUTDOWN_CHECK_INTERVAL: Duration = Duration::from_millis(200);

pub(super) struct ThreadedPoolImpl {
    /// Only touched from the worker thread(s).
    ///
    /// By design, tasks are not tuched from other threads.
    tasks: Mutex<HashMap<UniqueId, Weak<dyn TaskImmutable>>>,

    command_sender: chan::Sender<Command>,
    command_receiver: chan::Receiver<Command>,

    /// Work happens on this thread pool.
    ///
    /// This always has at least one thread.
    thread_pool: rayon::ThreadPool,

    /// Used to wake this worker pool up as audio ticks advance.
    audio_tick_counter: MpscCounter,
}

impl ThreadedPoolImpl {
    pub(super) fn new(threads: NonZeroUsize) -> Arc<Self> {
        let (command_sender, command_receiver) = chan::unbounded();

        let implementation = ThreadedPoolImpl {
            audio_tick_counter: MpscCounter::new(0),
            tasks: Mutex::new(HashMap::new()),
            command_sender,
            command_receiver,
            thread_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(threads.get())
                .build()
                .unwrap(),
        };

        let implementation = Arc::new(implementation);

        {
            let pool = Arc::downgrade(&implementation);
            std::thread::spawn(move || scheduling_thread(pool));
        }

        implementation
    }

    pub(super) fn signal_audio_tick_complete(&self) {
        self.audio_tick_counter
            .increment(NonZeroU64::new(1).unwrap());
    }

    pub(super) fn tick_work_impl(&self) {
        // Important: this mutex *must* be released by the time we get to Rayon.
        let work = {
            let mut work_map = self.tasks.lock().unwrap();

            // While we have new commands, execute them.
            while let Ok(cmd) = self.command_receiver.try_recv() {
                match cmd {
                    Command::NewWork { id, work } => {
                        let old = work_map.insert(id, work);
                        assert!(old.is_none(), "Attempt to double-register task");
                    }
                }
            }

            // Turn the hashmap into a vec of work items for sorting.
            //
            // We can optimize this later to not re-allocate all the time, but in the grand scheme of things this is nothing
            // compared to file I/O.
            let mut work: Vec<(UniqueId, Arc<dyn TaskImmutable>)> =
                Vec::with_capacity(work_map.len());
            work.extend(work_map.drain().filter_map(|x| Some((x.0, x.1.upgrade()?))));

            // Sort our work by priority.
            work.sort_unstable_by_key(|w| w.1.priority());
            work
        };

        // For now, we assume all work will execute and, consequently, that all work will be "late" if an audio tick is
        // missed. We will be smarter about this in the future if that is required.
        self.thread_pool.install(move || {
            work.into_par_iter()
                .filter_map(|(id, task)| {
                    if task.execute() {
                        Some((id, task))
                    } else {
                        None
                    }
                })
                .for_each(|(id, work)| {
                    self.tasks.lock().unwrap().insert(id, Arc::downgrade(&work));
                });
        });
    }

    pub(crate) fn register_task<T: Task>(&self, task: T) -> TaskHandle {
        let task_strong: Arc<dyn TaskImmutable> = Arc::new(AtomicRefCell::new(task));

        self.command_sender
            .send(Command::NewWork {
                id: UniqueId::new(),
                work: Arc::downgrade(&task_strong),
            })
            .expect("This channel is neither bounded nor closed");

        TaskHandle { task_strong }
    }
}

enum Command {
    NewWork {
        id: UniqueId,
        work: Weak<dyn TaskImmutable>,
    },
}

/// Scheduling thread for the worker pool.
fn scheduling_thread(pool: Weak<ThreadedPoolImpl>) {
    let mut audio_tick_prev = 0;
    let mut audio_tick_new = 0;
    let mut first = true;

    log::info!("Started background scheduling thread");
    while let Some(pool) = pool.upgrade() {
        let deadline = Instant::now() + SHUTDOWN_CHECK_INTERVAL;

        // Only do something if the audio tick advanced. Also, be careful that we do work for the first tick.
        //
        // This is required because (1) our timeout may expire before a tick, and (2) just checking the version will
        // skip the first one.  If it weren't for tests we wouldn't have to care about thi, but we need a strict
        // guarantee that n signals means n iterations in order to test.  Otherwise slower CI setups such as GitHub
        // actions start seeing spurious failures.
        if audio_tick_prev != audio_tick_new || first {
            pool.tick_work_impl();
            audio_tick_prev = audio_tick_new;
            first = false;
        }

        audio_tick_new = pool
            .audio_tick_counter
            .wait_deadline(audio_tick_prev, deadline)
            .unwrap_or(audio_tick_prev);
    }

    log::info!("Exiting scheduling thread because the worker pool is shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicBool, AtomicU64};
    use std::thread::sleep;

    /// A task which works by incrementing a counter every time it runs.
    struct CounterTask {
        counter: Arc<AtomicU64>,

        /// This is what is returned from execute; used in the tests to make sure tasks will stop.
        execute_ret: Arc<AtomicBool>,

        /// This is set when the task drops, so that we can test dropping as opposed to just not executing.
        is_alive: Arc<AtomicBool>,
    }

    impl Task for CounterTask {
        fn execute(&mut self) -> bool {
            self.counter.fetch_add(1, Ordering::Relaxed);
            self.execute_ret.load(Ordering::Relaxed)
        }

        fn priority(&self) -> TaskPriority {
            // For now, we don't do anything with priority.  When we do, we will make these tests more advanced.
            TaskPriority::Decoding(0)
        }
    }

    impl std::ops::Drop for CounterTask {
        fn drop(&mut self) {
            self.is_alive.store(false, Ordering::Relaxed);
        }
    }

    struct TestContext {
        /// Counters which will be incremented.
        counters: Vec<Arc<AtomicU64>>,

        /// The tasks, not yet registered.
        ///
        /// It is important that the test control registration since, for the test which tests the "real" pool, it's critical that the test control when they get registered.
        tasks: Vec<CounterTask>,

        /// The return values for the tasks.
        execute_rets: Vec<Arc<AtomicBool>>,

        alive_flags: Vec<Arc<AtomicBool>>,

        pool: Arc<ThreadedPoolImpl>,
    }

    impl TestContext {
        fn new(num_tasks: usize, num_threads: NonZeroUsize) -> TestContext {
            let mut tasks = vec![];
            let mut counters = vec![];
            let mut execute_rets = vec![];
            let mut alive_flags = vec![];

            for _ in 0..num_tasks {
                let counter = Arc::new(AtomicU64::new(0));
                let execute_ret = Arc::new(AtomicBool::new(true));
                let is_alive = Arc::new(AtomicBool::new(true));
                let task = CounterTask {
                    counter: counter.clone(),
                    execute_ret: execute_ret.clone(),
                    is_alive: is_alive.clone(),
                };
                tasks.push(task);
                counters.push(counter);
                execute_rets.push(execute_ret);
                alive_flags.push(is_alive);
            }

            let pool = ThreadedPoolImpl::new(num_threads);
            TestContext {
                counters,
                tasks,
                execute_rets,
                alive_flags,
                pool,
            }
        }

        /// Get a vec of the counter values.
        fn counter_vec(&self) -> Vec<u64> {
            self.counters
                .iter()
                .map(|x| x.load(Ordering::Relaxed))
                .collect()
        }

        fn alive_flags_vec(&self) -> Vec<bool> {
            self.alive_flags
                .iter()
                .map(|x| x.load(Ordering::Relaxed))
                .collect()
        }

        fn stop_task(&self, task_index: usize) {
            self.execute_rets[task_index].store(false, Ordering::Relaxed);
        }
    }

    /// Test the pool using the scheduling thread.
    ///
    /// This test is less advanced than the inline test; it mostly serves to prove that the
    /// scheduling thread advances.
    #[test]
    fn test_pool_threaded() {
        // This pool runs the scheduling thread.
        let mut context = TestContext::new(3, NonZeroUsize::new(2).unwrap());

        // Each time through the following loop, we will register one additional task. This has the effect of making it
        // such that the counters are like [3,2,1] at the end.  We can just check it there rather than at every
        // iteration.
        //
        // The first registered task--if it registers fast enough--may run twice because the pool is careful to run for
        // the zeroth tick, so we have to unfortunately check that too.
        let mut handles = vec![];
        for t in std::mem::take(&mut context.tasks) {
            handles.push(context.pool.register_task(t));
            context.pool.signal_audio_tick_complete();
            // Now we must sleep a little bit so that the pool has a chance to pick it up and run it.
            sleep(Duration::from_millis(100));
        }
        let cvec = context.counter_vec();
        if cvec[0] == 4 {
            // The first task ran twice.
            assert_eq!(cvec, vec![4, 2, 1]);
        } else {
            assert_eq!(cvec, vec![3, 2, 1]);
        }
    }
}

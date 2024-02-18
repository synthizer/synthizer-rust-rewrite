//! A module that lets us somewhat intelligently do background work to cooperate with the audio thread.
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

use crate::unique_id::UniqueId;

/// The worker pool's background threads will wake up this often to find out if the pool has gone away.
const SHUTDOWN_CHECK_INTERVAL: Duration = Duration::from_millis(200);

/// Priorities of a task. Lower is higher priority.
#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Debug, Hash)]
pub(crate) enum TaskPriority {
    /// This task is decoding a source.  The priority of that decoding is the u64 value, where 0 is highest priority.
    Decoding(u64),
}

/// trait representing tasks which may be scheduled to a pool.
pub(crate) trait Task: Send + Sync + 'static {
    /// What is the priority of this task?
    ///
    /// Tasks with lower priorities are ticked less often.
    ///
    /// This is permitted to change between runs.
    fn priority(&self) -> TaskPriority;

    /// Execute this task.
    ///
    /// If this function returns true, the task is given a chance to run again approximately at the next audio tick.
    /// Otherwise, it is assumed to be a one-off task and will be dropped from further processing.
    ///
    /// Tasks are not guaranteed to be ticked every audio update.  In particular, low priority tasks are ticked less
    /// often if work is running behind. Consequently, this should do as much work as it can, not just enough work for
    /// one audio tick.  That said, this scheduler is somewhat aware of the requirements, e.g. that if we tick a
    /// streaming source lesss than every 50 ms glitching happens.
    fn execute(&mut self) -> bool;
}

/// This is an internal trait which is implemented for `AtomicRefCell` to let tasks be mutable, while exposing immutable
/// interfaces for `Arc`.
///
/// It works by "unwrapping" and then calling into the inner trait, and lets us avoid having to double-box.
trait TaskImmutable: Send + Sync + 'static {
    fn priority(&self) -> TaskPriority;
    fn execute(&self) -> bool;
}

impl<T: Task> TaskImmutable for AtomicRefCell<T> {
    fn priority(&self) -> TaskPriority {
        self.borrow().priority()
    }

    fn execute(&self) -> bool {
        self.borrow_mut().execute()
    }
}

/// A pool of work to be done.
///
/// This is a bit like Rayon or insert your other favorite worker pool solution, except it's written to be able to run
/// inline so that users may ask Synthizer for samples.  To do so, it has the ability to block until all outstanding
/// work is consumed.  We also specialize it to support a fixed set of task types; we know what Synthizer needs, and can
/// e.g. do slightly better with scheduling than a generic solution.  We don't need to also support super generic
/// things.
///
/// Servers are injected into tasks by having the tasks themselves hold a reference as needed.  This allows better
/// testability.  Tasks should hold a weak reference to the server (`std::sync::Weak`) as to prevent a circular
/// reference, and then do nothing if that reference is dead.  In general, it's somewhat rare for tasks to need the
/// server directly.
///
/// The only operation which may safely be performed from a real audio thread is `signal_audio_tick_complete`.  All
/// other operations may block, primarily due to memory allocation and secondarily due to our choice to use dependencies
/// we don't control.
///
/// Note that all queues are unbounded by design. If they were bounded, then it would be possible to end up blocking in
/// various high-priority places or on the user's thread.  The bound is implicit instead, in that each task is only
/// registered once and so the memory usage is `O(tasks_registered)`.  Put more simply, (de)registering work never
/// blocks under the assumption that manipulating work is `O(max(outstanding tasks))` in terms of the length of the
/// queue.
#[derive(Clone)]
pub(crate) struct WorkerPoolHandle {
    /// Only these handles hold a strong reference; when the last strong reference goes away, the pool will shut down.
    implementation: Arc<WorkerPoolImpl>,
}

struct WorkerPoolImpl {
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

/// A task is alive as long as the handle to it is.
///
/// When this handle is dropped, the task on the pool is likewise cancelled.  That is:
///
/// - If the task ever returns false in [Task::execute] it will not be re-scheduled but the object is kept alive.
/// - If this handle is dropped, the task will be cancelled and the underlying object will likewise go away.
///
/// because [Task::execute] allows mutable self access, tasks should work out their own methods of communication.
pub(crate) struct TaskHandle {
    task_strong: Arc<dyn TaskImmutable>,
}

impl WorkerPoolHandle {
    /// Spawn a worker pool with the given number of background threads.
    ///
    /// If `runs_inline` is true, then this pool is assumed to be for synthesis that is producing samples consumed by
    /// the user on a non-audio thread, and no scheduling thread will be spawned.
    pub(crate) fn new(threads: NonZeroUsize, runs_inline: bool) -> Self {
        let (command_sender, command_receiver) = chan::unbounded();

        let implementation = WorkerPoolImpl {
            audio_tick_counter: MpscCounter::new(0),
            tasks: Mutex::new(HashMap::new()),
            command_sender,
            command_receiver,
            thread_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(threads.get())
                .build()
                .unwrap(),
        };

        let handle = WorkerPoolHandle {
            implementation: Arc::new(implementation),
        };

        if !runs_inline {
            let pool = Arc::downgrade(&handle.implementation);
            std::thread::spawn(move || scheduling_thread(pool));
        }

        handle
    }

    /// Tell the pool that a tick of audio data has just finished, and it should start any work that it thinks it will
    /// need to fulfill the next tick.
    ///
    /// If the pool's dispatcher thread is running, the dispatcher thread will then call `tick_work` in the background,
    /// dispatching tasks to our thread pool, then go back to sleep.
    pub(crate) fn signal_audio_tick_complete(&self) {
        self.implementation.signal_audio_tick_complete_impl()
    }

    /// Tick this thread pool, running work inline.
    ///
    /// This function should not be called on more than one thread at once, nor should it be called if a schedule thread
    /// is running. It is exposed to other modules in this crate so that it is possible for a user who is using
    /// Synthizer to generate samples have work happen at an appropriate time.  Violating this requirement probably
    /// either causes a deadlock, but the absolute best case is double-execution of work.
    pub(crate) fn tick_work(&self) {
        self.implementation.tick_work_impl()
    }

    /// Register a task with this thread pool.
    ///
    /// This function will allocate.
    ///
    /// The returned handle must not be dropped for as long as the task should be running.
    #[must_use = "Dropping task handles immediately cancels tasks, so they will likely never run unless the first run is fast enough to race this drop"]
    pub(crate) fn register_task<T: Task>(&self, task: T) -> TaskHandle {
        self.implementation.register_work_impl(task)
    }
}

impl WorkerPoolImpl {
    fn signal_audio_tick_complete_impl(&self) {
        self.audio_tick_counter
            .increment(NonZeroU64::new(1).unwrap());
    }

    fn tick_work_impl(&self) {
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

    pub(crate) fn register_work_impl<T: Task>(&self, task: T) -> TaskHandle {
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
fn scheduling_thread(pool: Weak<WorkerPoolImpl>) {
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

    /// We will run two tests, one which ticks the pool in the foreground and one which ticks the pool in the
    /// background.  Both tests need a pool, some counters, and some registered tasks.
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

        pool: WorkerPoolHandle,
    }

    impl TestContext {
        fn new(num_tasks: usize, num_threads: NonZeroUsize, runs_inline: bool) -> TestContext {
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

            let pool = WorkerPoolHandle::new(num_threads, runs_inline);
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

    #[test]
    fn test_pool_inline() {
        let mut context = TestContext::new(3, NonZeroUsize::new(2).unwrap(), true);

        // before doing anything, try running the pool with no tasks. This could in theory detect a crash as the imoplementation becomes more advanced.
        context.pool.signal_audio_tick_complete();
        context.pool.tick_work();

        // Register the tasks, signal some work, then wait for a little bit; if this pool is truly inline, no tasks will
        // run.
        let mut handles = vec![];
        for i in std::mem::take(&mut context.tasks) {
            handles.push(context.pool.register_task(i));
        }

        context.pool.signal_audio_tick_complete();

        sleep(Duration::from_millis(200));
        assert_eq!(context.counter_vec(), vec![0, 0, 0]);

        // Ticking once should always increment an inline pool's tasks once.
        context.pool.tick_work();
        assert_eq!(context.counter_vec(), vec![1, 1, 1]);
        assert_eq!(context.alive_flags_vec(), vec![true, true, true]);

        // tell one of the tasks to stop.
        context.stop_task(1);
        context.pool.signal_audio_tick_complete();
        context.pool.tick_work();
        // It will take two ticks for it to actually go anywhere.
        assert_eq!(context.counter_vec(), vec![2, 2, 2]);
        context.pool.signal_audio_tick_complete();
        context.pool.tick_work();
        assert_eq!(context.counter_vec(), vec![3, 2, 3]);

        // Dropping all task handles should immediately drop tasks, regardless what the pool thinks.
        std::mem::drop(handles);
        std::mem::drop(context.pool);
        let avec = context
            .alive_flags
            .iter()
            .map(|x| x.load(Ordering::Relaxed))
            .collect::<Vec<bool>>();
        // Partially dropped the struct, so we can't use the helper methods anymore.
        assert_eq!(avec, vec![false, false, false]);
    }

    /// Test the pool using the scheduling thread.
    ///
    /// This test is less advanced because the logic is the same either way; it mostly serves to prove that the
    /// scheduling thread advances.
    #[test]
    fn test_pool_background() {
        // This pool runs the scheduling thread.
        let mut context = TestContext::new(3, NonZeroUsize::new(2).unwrap(), false);

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

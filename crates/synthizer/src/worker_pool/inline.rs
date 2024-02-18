//! An inline thread pool.
//!
//! This thread pool is a simpler pool which works by just having a hashmap of tasks that it iterates over.  The pool
//! uses mutexes and is not realtime-safe because it is intended for the use case of users which are trying to use
//! Synthizer as a synthesizer that just gives out samples.
use std::collections::HashMap;

use std::sync::{Arc, Mutex, Weak};

use atomic_refcell::AtomicRefCell;

use crate::unique_id::UniqueId;

use super::*;

struct SortedTaskEntry {
    task: Arc<dyn TaskImmutable>,
    priority: TaskPriority,
    id: UniqueId,

    /// Shuffles tasks of the same priority by breaking ties in the sort.
    random_seed: u8,
}

struct InlinePoolState {
    tasks: HashMap<UniqueId, Weak<dyn TaskImmutable>>,

    /// Temporary vec we sort to to prevent spamming the allocator.
    sorted_task_vec: Vec<SortedTaskEntry>,
}

pub(super) struct InlinePoolImpl {
    state: Mutex<InlinePoolState>,
}

impl InlinePoolImpl {
    pub(super) fn new() -> Arc<Self> {
        let state = InlinePoolState {
            tasks: HashMap::with_capacity(32),
            sorted_task_vec: Vec::with_capacity(32),
        };

        Arc::new(InlinePoolImpl {
            state: Mutex::new(state),
        })
    }

    pub(super) fn register_task_impl<T: Task>(&self, task: T) -> TaskHandle {
        let id = UniqueId::new();

        let mut state = self.state.lock().unwrap();

        let immutable: Arc<dyn TaskImmutable> = Arc::new(AtomicRefCell::new(task));
        state.tasks.insert(id, Arc::downgrade(&immutable));

        TaskHandle {
            task_strong: immutable,
        }
    }

    pub(super) fn run_tasks(&self) {
        let mut state = self.state.lock().unwrap();
        // For split borrows.
        let state: &mut InlinePoolState = &mut state;

        // We respect priority even though every tick always runs all tasks.  This at least approximates the determinism
        // which we receive from the multithreaded pool.  TO get something more resembling threads we shuffle based off
        // random integers.

        state
            .sorted_task_vec
            .extend(state.tasks.iter().filter_map(|(id, task_weak)| {
                let upgraded = task_weak.upgrade()?;
                let priority = upgraded.priority();
                Some(SortedTaskEntry {
                    id: *id,
                    task: upgraded,
                    priority,
                    random_seed: rand::random(),
                })
            }));

        state
            .sorted_task_vec
            .sort_by_key(|t| (t.priority, t.random_seed));

        // We want to remove from the hashmap when tasks say they're done.  We also want to clear the vector. Ergo drain, and a manual remove.
        for task in state.sorted_task_vec.drain(..) {
            if !task.task.execute() {
                state.tasks.remove(&task.id);
            }
        }

        // Now get rid of any tasks which aren't around anymore.
        state.tasks.retain(|_, task| task.strong_count() != 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
        tasks: Vec<CounterTask>,

        /// The return values for the tasks.
        execute_rets: Vec<Arc<AtomicBool>>,

        alive_flags: Vec<Arc<AtomicBool>>,

        pool: Arc<InlinePoolImpl>,
    }

    impl TestContext {
        fn new(num_tasks: usize) -> TestContext {
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

            let pool = InlinePoolImpl::new();
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
        let mut context = TestContext::new(3);

        // before doing anything, try running the pool with no tasks. This could in theory detect a crash as the imoplementation becomes more advanced.
        context.pool.run_tasks();

        let mut handles = vec![];
        for i in std::mem::take(&mut context.tasks) {
            handles.push(context.pool.register_task_impl(i));
        }

        // Ticking once should always increment an inline pool's tasks once.
        context.pool.run_tasks();
        assert_eq!(context.counter_vec(), vec![1, 1, 1]);
        assert_eq!(context.alive_flags_vec(), vec![true, true, true]);

        // tell one of the tasks to stop.
        context.stop_task(1);
        context.pool.run_tasks();

        // It runs for one more tick.
        assert_eq!(context.counter_vec(), vec![2, 2, 2]);
        context.pool.run_tasks();

        assert_eq!(context.counter_vec(), vec![3, 2, 3]);

        // Dropping all task handles should immediately drop tasks.
        std::mem::drop(handles);
        context.pool.run_tasks();
        assert_eq!(context.alive_flags_vec(), vec![false, false, false]);
    }
}

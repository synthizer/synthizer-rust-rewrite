//! A module that lets us somewhat intelligently do background work to cooperate with the audio thread.
mod inline;
mod threaded;

use std::num::NonZeroUsize;

use std::sync::Arc;

use threaded::*;

/// trait representing tasks which may be scheduled to a pool.
pub(crate) trait Task: Send + 'static {
    /// Execute this task.
    ///
    /// If this function returns true, the task is given a chance to run again approximately at the next audio tick.
    /// Otherwise, it is assumed to be a one-off task and will be dropped from further processing.
    ///
    /// Tasks are not guaranteed to be ticked every audio update. Consequently, this should do as much work as it can,
    /// not just enough work for one audio tick. That said, this scheduler is somewhat aware of the requirements,
    /// e.g. that if we tick a streaming source less than every 50 ms glitching happens.
    fn execute(&mut self) -> bool;
}

#[derive(Clone)]
enum WorkerPoolKind {
    Inline(Arc<inline::InlinePoolImpl>),
    Threaded(Arc<threaded::ThreadedPoolImpl>),
}

/// A pool of work to be done.
///
/// This is a bit like Rayon or insert your other favorite worker pool solution, except it's written to optionally be
/// able to run inline on the current thread so that users may ask Synthizer for samples.  We also specialize it to
/// support a fixed set of task types; we know what Synthizer needs, and can e.g. do slightly better with scheduling
/// than a generic solution.  We don't need to also support super generic things.
///
/// Servers are injected into tasks by having the tasks themselves hold a reference as needed.  This allows better
/// testability, since tests for pools themselves needn't concern themselves with getting a server from somewhere. Tasks
/// should hold a weak reference to the server (`std::sync::Weak`) as to prevent a circular reference, and then do
/// nothing if that reference is dead.  In general, it's somewhat rare for tasks to need the server directly.
///
/// The only operation which may safely be performed from a real audio thread is `signal_audio_tick_complete`.  All
/// other operations may block, primarily due to memory allocation and secondarily due to our choice to use dependencies
/// we don't control.  Note further that `signal_audio_tick_complete` will choose to run work on the calling thread if
/// this pool was inline, rendering any use of this handle on an audio thread non-realtime-safe if it was created with
/// `new_inline`.
///
/// Note that the multithreaded pool uses unbounded command queues. If they were bounded, then it would be possible to
/// end up blocking in various high-priority places or on the user's thread.  The bound is implicit instead, in that
/// each task is only registered once and so the memory usage is `O(tasks_registered)`.  Put more simply,
/// (de)registering work never blocks under the assumption that manipulating work is `O(max(outstanding tasks))` in
/// terms of the length of the queue.  The simpler inline pool always registers directly via a mutex, so this is a
/// nonissue for that variation.
#[derive(Clone)]
pub(crate) struct WorkerPoolHandle {
    kind: WorkerPoolKind,
}

impl WorkerPoolHandle {
    /// Spawn a worker pool with the given number of background threads.
    pub(crate) fn new_threaded(threads: NonZeroUsize) -> Self {
        let implementation = ThreadedPoolImpl::new(threads);

        WorkerPoolHandle {
            kind: WorkerPoolKind::Threaded(implementation),
        }
    }

    /// Spawn a worker pool which runs work inline as audio ticks complete.
    pub(crate) fn new_inline() -> Self {
        let implementation = inline::InlinePoolImpl::new();

        WorkerPoolHandle {
            kind: WorkerPoolKind::Inline(implementation),
        }
    }

    /// Tell the pool that a tick of audio data has just finished, and it should start any work that it thinks it will
    /// need to fulfill the next tick.
    ///
    /// If the pool is a threaded pool, this wakes the background threads. If it is inline, this instead executes all
    /// outstanding work on the current thread.
    pub(crate) fn signal_audio_tick_complete(&self) {
        match &self.kind {
            WorkerPoolKind::Inline(p) => p.run_tasks(),
            WorkerPoolKind::Threaded(p) => p.signal_audio_tick_complete(),
        }
    }

    /// Register a task with this thread pool.
    ///
    /// This function will allocate.
    ///
    /// The task will run until it returns false from execute().
    pub(crate) fn register_task<T: Task>(&self, task: T) {
        match &self.kind {
            WorkerPoolKind::Inline(p) => p.register_task_impl(task),
            WorkerPoolKind::Threaded(p) => p.register_task(task),
        }
    }
}

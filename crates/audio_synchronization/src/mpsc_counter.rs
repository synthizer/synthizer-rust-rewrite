use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::num::NonZeroU64;
use std::time::{Duration, Instant};

use crate::sync::atomic::{AtomicU64, Ordering};
use crate::sync::{spin_loop, Thread};

/// A counter which can be waited on
///
/// Waiting threads may ask this counter for a value.  They may also wait until this counter's value changes.
/// Incrementing threads will wake the counter.  The maximum value is `u64::MAX / 2`, at which point the app crashes
/// with a panic.  Only incrementing the counter is soft-realtime safe.
///
/// Only the first thread which waits on the counter may wait.  If more than one thread tries to wait, a panic usually
/// results (there is some internal spinning; during that phase we cannot detect other waiters, but handle them
/// correctly).  This is MPSC but can be done soundly while still allowing it to be stored inline, so thread handles are
/// stored implicitly as threads wait rather than stored up front.  This counter may be incremented by any number of
/// threads simultaneously, but doing so will result in many spurious wakes.
///
/// Incrementing threads are soft realtime safe. Waiting threads (even with a timeout) may wait forever.
///
/// The caller will need to be able to hold onto previously read values in order to drive this counter.  Correct usage
/// is to start with a previous value of 0, then continually call `wait_*` (alternatively use `get` for a first value,
/// or whatever you initialized it with).
///
/// # Implementation and Realtime Safety
///
/// This  uses Rust's thread parking facilities.  The incrementing thread unparks if there is a waiter.  This is not
/// documented to be realtime-safe, but a read of the Rust stdlib source and correlation with documentation indicates
/// the following:
///
/// On Windows, this is `WaitOnAddress` and friends.  Windows APIs all like to use these functions rather than
/// callbacks(e.g. WASAPI and `WaitForSingleObject`).  There is little question as to whether or not this is safe there.
///
/// On Linux, Android, and all other platforms supporting a futex, parking is done with afutex.  Linux realtime-safety
/// is an interesting topic that boils down to "set scheduling priorities properly and hope" to some extent, but[this
/// kernel documentation](https://docs.kernel.org/locking/pi-futex.html) specifically calls out audio applications.
/// Since Android is using the Linux kernel, we assume this is good enough there as well; it would be surprising in the
/// extreme if they modified it to specifically break any guarantees futexes might have.  Note that (1) Rust isn't using
/// PI-futex, but that (2) realtime threads only wake, so the priority inversion concerns are thus avoided.
///
/// On Apple platforms, Rust parking is dispatch_semaphore_t.  Surprisingly, Apple doesn't provide easy to find
/// documentation on what it is and isn't safe to do from Core Audio (indeed there is almost no docs for it outside the
/// headers themselves), but explicitly supports multithreaded synthesis and occasionally says dispatch_semaphore_t is
/// the thing to use in one-off comments and such in examples.
///
/// The risk here is that Rust could in theory migrate away from these APIs.  If that happens, we can reimplement
/// specific parking primitives ourselves pretty easily, just by copying old code out of the rust stdlib in the worst
/// case.  If you find some instance where this counter is being problematic, you're encouraged to open an issue.
pub struct MpscCounter {
    /// the packed state.
    ///
    /// The high bit is set if there is a thread that needs unparking.  The low 63 bits are the counter.
    state: AtomicU64,

    /// This is set to a thread handle for the first thread which waits, and is valid if and only if the high bit of
    /// state is set.
    thread_handle: UnsafeCell<MaybeUninit<Thread>>,
}

/// Internal state of the counter.
///
/// This is packed/unpacked from a u64.
#[derive(Copy, Clone, Debug)]
struct State {
    /// The high bit of the u64 is whether a thread handle exists.
    thread_initialized: bool,

    /// The low 63 bits are the counter.
    counter: u64,
}

impl State {
    fn unpack(val: u64) -> State {
        let thread_initialized = (val >> 63) != 0;
        let counter = val & !(1 << 63);
        Self {
            thread_initialized,
            counter,
        }
    }

    fn pack(&self) -> u64 {
        ((self.thread_initialized as u64) << 63) | self.counter
    }

    /// Increment the counter or panic if we hit `u64::MAX / 2`.
    #[must_use]
    pub fn increment(&self, amount: u64) -> State {
        let mut ns = *self;
        ns.counter += amount;
        assert!(ns.counter <= u64::MAX / 2, "Counter has overflowed");
        ns
    }
}

impl MpscCounter {
    pub fn new(initial_value: u64) -> Self {
        assert!(
            initial_value <= u64::MAX / 2,
            "The counter must never be over u64::MAX / 2"
        );

        Self {
            state: AtomicU64::new(initial_value),
            thread_handle: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    /// Read the value of this counter.
    ///
    /// This may be called from any number of threads simultaneously.
    pub fn get(&self) -> u64 {
        State::unpack(self.state.load(Ordering::Acquire)).counter
    }

    /// Wait on this counter to change from a previously observed value using a spinloop.
    ///
    /// This may also be called from many threads simultaneously.  After a very short spinloop, it will return `None`.
    pub fn wait_spinning(&self, previous: u64) -> Option<u64> {
        for _ in 0..3 {
            let state = State::unpack(self.state.load(Ordering::Acquire));
            if state.counter > previous {
                return Some(state.counter);
            }

            spin_loop();
        }

        None
    }

    /// Wait for this counter to change from a previously observed value.
    pub fn wait(&self, previous: u64) -> u64 {
        self.wait_internal(previous, || {
            crate::sync::park();
            true
        })
        .expect("Waiting forever should always return a value")
    }

    /// Wait on this counter to change until the specified timeout elapses.
    ///
    /// The expression `Instant::now() + timeout` must be valid (e.g. `Duration::MAX` will crash).
    #[cfg(not(loom))]
    pub fn wait_timeout(&self, previous: u64, timeout: Duration) -> Option<u64> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .expect("Unable to compute a deadline from the timeout");
        self.wait_deadline(previous, deadline)
    }

    /// Wait on this counter to change until the specified deadline.
    #[cfg(not(loom))]
    pub fn wait_deadline(&self, previous: u64, deadline: Instant) -> Option<u64> {
        self.wait_internal(previous, || {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }

            let timeout = deadline - now;
            // This is what doesn't work with Loom.
            std::thread::park_timeout(timeout);
            true
        })
    }

    /// Internal implementation of waiting; `maybe_park` should park and return true until such time as the thread can't park
    /// anymore, e.g. because of a timeout.
    ///
    /// Always returns `Some` if `maybe_park` doesn't return true.  May return `None` if `maybe_park` returns false, but may
    /// sometimes pick up a final update.
    fn wait_internal(&self, previous: u64, mut maybe_park: impl FnMut() -> bool) -> Option<u64> {
        if let Some(x) = self.wait_spinning(previous) {
            return Some(x);
        }

        // Did we yet validate that the user isn't trying to be MPMC?
        let mut parked_handle_checked = false;

        loop {
            let mut state = State::unpack(self.state.load(Ordering::Acquire));

            // If the thread isn't initialized yet, we must do that.  Furthermore we must also not timeout here--the CAS
            // loop below must succeed. Otherwise, we can leak a thread handle.  For any reasonable timeout value, this
            // is fine.
            if !state.thread_initialized {
                // We only need to copy the handle in once.
                unsafe {
                    self.thread_handle
                        .get()
                        .as_mut()
                        .unwrap_unchecked()
                        .write(crate::sync::current());
                }

                // But this CAS loop must succeed.
                loop {
                    let mut new_state = state;
                    new_state.thread_initialized = true;
                    match self.state.compare_exchange(
                        state.pack(),
                        new_state.pack(),
                        Ordering::Acquire,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => {
                            state = new_state;
                            break;
                        }
                        Err(s) => state = State::unpack(s),
                    }
                }
            }

            // Ok. We have a thread in the handle. But is it us?
            if !parked_handle_checked {
                let contained_handle = unsafe { self.get_contained_handle() };
                assert_eq!(contained_handle.id(), crate::sync::current().id());
                parked_handle_checked = true;
            }

            // Cool, now is it the case that the state's counter isn't what we expected?
            if state.counter != previous {
                return Some(state.counter);
            }

            // Otherwise, park the thread if possible.
            if !maybe_park() {
                return None;
            }
        }
    }

    unsafe fn get_contained_handle(&self) -> &Thread {
        unsafe {
            self.thread_handle
                .get()
                .as_ref()
                .unwrap_unchecked()
                .as_ptr()
                .as_ref()
                .unwrap_unchecked()
        }
    }

    /// Increment this counter by the specified amount.
    ///
    /// `amount` must be non-zero.
    ///
    /// Calling this function from multiple threads is supported but will result in spurious wake-ups if those calls
    /// overlap as well as spinloops.
    ///
    /// Returns the new value after the increment.
    pub fn increment(&self, amount: NonZeroU64) -> u64 {
        let mut state = State::unpack(self.state.load(Ordering::Relaxed));

        loop {
            let new_state = state.increment(amount.get());

            // Now we store this state if we can.
            //
            // States are only changed by other incrementing threads (to the new incremented value) or by the reader (to
            // the same value, but with an initialized thread handle).
            match self.state.compare_exchange(
                state.pack(),
                new_state.pack(),
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    state = new_state;
                    break;
                }
                Err(s) => {
                    if state.thread_initialized {
                        // Assert that one can never deinit the thread.
                        debug_assert_eq!(state.thread_initialized, s & (1 << 63) != 0);
                    }
                    state = State::unpack(s);
                }
            }
        }

        // If the state we now have (after the increment) had an initialized thread handle, then we can unpark.
        if state.thread_initialized {
            unsafe { self.get_contained_handle().unpark() }
        }

        state.counter
    }
}

unsafe impl Send for MpscCounter {}
unsafe impl Sync for MpscCounter {}

impl Drop for MpscCounter {
    fn drop(&mut self) {
        let state = State::unpack(*self.state.get_mut());
        if state.thread_initialized {
            unsafe {
                self.thread_handle
                    .get()
                    .as_mut()
                    .unwrap_unchecked()
                    .assume_init_drop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sync::*;

    /// Spawn some number of writers and increment the counter by some amount.  Then, spawn a reader thread that
    /// verifies that we reach that amount.
    fn test_waiting_impl(num_writers: usize, increment: u64) {
        // Each writer will write 3 times.
        const NUM_WRITES: usize = 3;

        // The final value we expect.
        let target = num_writers as u64 * (NUM_WRITES as u64) * increment;

        let mut write_handles: Vec<JoinHandle<()>> = vec![];
        let counter: std::sync::Arc<MpscCounter> = std::sync::Arc::new(MpscCounter::new(0));

        for _ in 0..num_writers {
            let counter = counter.clone();
            let jh = spawn(move || {
                for _ in 0..NUM_WRITES {
                    counter.increment(NonZeroU64::new(increment).unwrap());
                }
            });

            write_handles.push(jh);
        }

        let final_join_handle = spawn(move || {
            let mut prev = 0;
            while prev < target {
                prev = counter.wait(prev);
            }

            prev
        });

        for h in write_handles {
            h.join().unwrap();
        }

        assert_eq!(final_join_handle.join().unwrap(), target);
    }

    #[test]
    fn wait_writers1_increment1() {
        wrap_test(|| test_waiting_impl(1, 1));
    }

    #[test]
    fn wait_writers2_increment1() {
        wrap_test(|| test_waiting_impl(2, 1));
    }

    #[test]
    fn wait_writers2_increment10() {
        wrap_test(|| test_waiting_impl(2, 10));
    }
}

#[cfg(all(test, not(loom)))]
mod not_loom_tests {
    use super::*;

    #[test]
    fn timeout_eventually_returns() {
        let counter = MpscCounter::new(0);

        assert!(counter.wait_timeout(0, Duration::from_secs(1)).is_none());
    }

    #[test]
    fn deadline_eventually_returns() {
        let counter = MpscCounter::new(0);
        assert!(counter
            .wait_deadline(0, Instant::now() + Duration::from_secs(1))
            .is_none());
    }
}

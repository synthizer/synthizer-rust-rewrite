use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

/// Make `T: Send` Sync.
///
/// This locks the value to one specific thread for the duration of the borrow, even for immutable borrows.  It's like
/// `AtomicRefCell`, but even simultaneous immutable borrows are prohibited.
///
/// It lets us have `T` use interior mutability while still being part of signal state.  Signals can then output borrows
/// to their owned values.
pub struct ExclusiveThreadCell<T> {
    inner: T,
    owning_thread: AtomicU64,
}

unsafe impl<T: Send> Sync for ExclusiveThreadCell<T> {}

pub struct ExclusiveThreadCellBorrow<'a, T> {
    reference: &'a ExclusiveThreadCell<T>,
}

impl<T> std::ops::Deref for ExclusiveThreadCellBorrow<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.reference.inner
    }
}

// Code to allocate thread ids.
//
// The problem with stdlib's is that it uses mutexes to try to handle wrapping. We don't need or want that.

/// First thread starts at 1, so we may reserve 0 to mean `None`.
static THREAD_COUNTER: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static THREAD_ID: Cell<u64> = const{Cell::new(0)};
}

fn get_thread_id() -> u64 {
    let cur = THREAD_ID.get();
    if cur != 0 {
        return cur;
    }

    let new = THREAD_COUNTER.fetch_add(1, Ordering::Relaxed);
    THREAD_ID.set(new);
    new
}

impl<T> ExclusiveThreadCell<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            owning_thread: AtomicU64::new(0),
        }
    }

    pub(crate) fn borrow(&self) -> ExclusiveThreadCellBorrow<'_, T> {
        let cur = self.owning_thread.load(Ordering::Relaxed);

        let good = if cur == 0 {
            // We can take this thread over.
            let tid = get_thread_id();

            self.owning_thread
                .compare_exchange(0, tid, Ordering::Relaxed, Ordering::Acquire)
                .is_ok()
        } else {
            cur == get_thread_id()
        };

        assert!(
            good,
            "Multiple accesses to ExclusiveThreadCell from different threads"
        );

        ExclusiveThreadCellBorrow { reference: self }
    }
}

impl<T> Drop for ExclusiveThreadCellBorrow<'_, T> {
    fn drop(&mut self) {
        self.reference.owning_thread.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exclusive_thread_cell_borrow() {
        let cell = ExclusiveThreadCell::new(42);
        {
            let borrow = cell.borrow();
            assert_eq!(*borrow, 42);
        }
        // Ensure the thread id is reset after the borrow is dropped
        assert_eq!(cell.owning_thread.load(Ordering::Relaxed), 0);
    }

    #[test]
    #[should_panic(expected = "Multiple accesses to ExclusiveThreadCell from different threads")]
    fn test_exclusive_thread_cell_borrow_panic() {
        let cell = std::sync::Arc::new(ExclusiveThreadCell::new(42));
        let _borrow = cell.borrow();

        let cell = cell.clone();
        if let Err(e) = std::thread::spawn(move || {
            let _borrow2 = cell.borrow();
        })
        .join()
        {
            std::panic::resume_unwind(e);
        }
    }

    #[test]
    fn test_thread_id_allocation() {
        let id1 = get_thread_id();
        let id2 = get_thread_id();
        assert_eq!(id1, id2);

        let handle = std::thread::spawn(move || {
            let id3 = get_thread_id();
            assert_ne!(id1, id3);
        });

        handle.join().unwrap();
    }
}

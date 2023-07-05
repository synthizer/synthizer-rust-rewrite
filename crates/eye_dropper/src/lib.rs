//!  Get visibility into if things have dropped, and how often.
//!
//! When writing data structures, it is sometimes important to test whether elements inside the data structure have
//! dropped.  This can manifest in three ways:
//!
//! - Testing how many drops total have occurred.
//! - Testing if specific items have been dropped
//! - Testing that double drops of the same memory do not happen.
//!
//! To solve this, create an [EyeDropper], which wraps some type `T` in a struct that tracks drops.  This returns a
//! [LocationHandle] which can be used to check whether the referenced location is alive or has been dropped, and a
//! [TrackedDrop] which contains the specified data and tracks drops.  Store [TrackedDrop] in the data structure to be
//! tested, then use the methods on [LocationHandle] and [EyeDropper] to check assertions.
use std::marker::PhantomData;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

#[derive(Debug, Default)]
struct EyeDropperCounters {
    drop_count: AtomicU64,
}

/// Returns handles which can track drops of a `T`.
#[derive(Debug)]
pub struct EyeDropper<T> {
    counts: Arc<EyeDropperCounters>,
    _phantom: PhantomData<*const T>,
}

#[derive(Default, Debug)]
struct DropTracker {
    dropped: AtomicBool,
}

#[derive(Debug)]
pub struct TrackedDrop<T> {
    data: T,
    /// The arc is gone if we double-drop, so we assert against this boolean which is more likely to still be in the
    /// memory location in question.
    previously_dropped: bool,
    tracker: Arc<DropTracker>,
    eye_dropper: Arc<EyeDropper<T>>,
}

pub struct LocationHandle<T> {
    tracker: Arc<DropTracker>,
    _phantom: PhantomData<*const T>,
}

unsafe impl<T: Send> Send for LocationHandle<T> {}
unsafe impl<T: Sync> Sync for LocationHandle<T> {}

impl<T> std::ops::Deref for TrackedDrop<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T> std::ops::DerefMut for TrackedDrop<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl<T> Drop for TrackedDrop<T> {
    fn drop(&mut self) {
        assert!(
            !self.previously_dropped,
            "AN attempt to drop the same memory twice has occurred"
        );
        self.tracker.dropped.store(true, Ordering::Relaxed);
        self.eye_dropper
            .counts
            .drop_count
            .fetch_add(1, Ordering::Relaxed);
    }
}

impl<T> EyeDropper<T> {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            counts: Default::default(),
            _phantom: PhantomData,
        })
    }

    /// Read how many drops have happened so far.
    ///
    /// In concurrent contexts, it is possible for a [LocationHandle] to be dropped before being reflected in this
    /// counter if a drop is occurring concurrently with asserting threads.
    fn get_drop_counter(&self) -> u64 {
        self.counts.drop_count.load(Ordering::Relaxed)
    }

    /// Assert that at least this many drops have happened so far.
    #[track_caller]
    pub fn assert_at_least(&self, drops: u64) {
        let cur = self.get_drop_counter();
        assert!(
            cur >= drops,
            "Expected {} drops but found only {}",
            drops,
            cur
        );
    }

    /// Assert that at most the specified number of drops have occurred.
    #[track_caller]
    pub fn assert_at_most(&self, drops: u64) {
        let cur = self.get_drop_counter();
        assert!(
            cur <= drops,
            "Expected at most {} drops but found {}",
            drops,
            cur
        );
    }

    /// Assert that the drop counter is in the given range.
    #[track_caller]
    pub fn assert_in_range(&self, range: impl std::ops::RangeBounds<u64>) {
        let cur = self.get_drop_counter();
        assert!(
            range.contains(&cur),
            "{} is not in range {:?} to {:?}",
            cur,
            range.start_bound(),
            range.end_bound()
        );
    }

    #[track_caller]
    pub fn assert_exact(&self, num: u64) {
        self.assert_in_range(num..=num);
    }

    /// Get a location handle and tracked drop.'
    pub fn new_value(self: &Arc<Self>, new_val: T) -> (LocationHandle<T>, TrackedDrop<T>) {
        let tracker = Arc::new(DropTracker::default());
        let tracked = TrackedDrop {
            data: new_val,
            previously_dropped: false,
            tracker: tracker.clone(),
            eye_dropper: self.clone(),
        };
        let loc = LocationHandle {
            tracker,
            _phantom: PhantomData,
        };
        (loc, tracked)
    }
}

impl<T> LocationHandle<T> {
    pub fn is_alive(&self) -> bool {
        !self.tracker.dropped.load(Ordering::Relaxed)
    }

    pub fn is_dropped(&self) -> bool {
        !self.is_alive()
    }

    /// Assert that this location is alive.
    #[track_caller]
    pub fn assert_alive(&self) {
        assert!(
            self.is_alive(),
            "The location tracked by this handle has been dropped"
        );
    }

    /// Assert that this location has been dropped.
    #[track_caller]
    pub fn assert_dropped(&self) {
        assert!(
            self.is_dropped(),
            "The location tracked by this handle has not yet been dropped"
        );
    }
}

unsafe impl<T> Send for EyeDropper<T> {}
unsafe impl<T> Sync for EyeDropper<T> {}

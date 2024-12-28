use std::sync::Arc;

use arc_swap::{ArcSwap, ArcSwapOption};

pub(crate) struct ArcStash<T> {
    next: ArcSwapOption<T>,
}

/// Any type implementing this trait may be used with [ArcConsumer].
pub(crate) trait GetArcStash: Sized {
    fn get_stash(&self) -> &ArcStash<Self>;
}

/// Like `ArcSwap`, but deferring frees.
///
/// The consumer loads, then `defer_reclaim`'s the value.  The value will not be reclaimed on the consumer's thread, as
/// long as that `Arc` is the last reference owned by this thread.
///
/// TLDR: don't use this on "owned" data. It's more like a queue, where you can miss updates or get older values.
///
/// As long as there is only one consumer, deferring is as waitfree as ArcSwap (e.g. for practical purposes always).
///
/// The point is not to enter the kernel from (possibly) audio threads.
pub(crate) struct DeferredArcSwap<T: GetArcStash + Send + Sync + 'static> {
    current: ArcSwap<T>,
    freelist: ArcSwapOption<T>,
}

impl<T: GetArcStash + Send + Sync + 'static> DeferredArcSwap<T> {
    pub(crate) fn new(initial_value: Arc<T>) -> Self {
        Self {
            current: ArcSwap::new(initial_value),
            freelist: ArcSwapOption::default(),
        }
    }

    pub(crate) fn load_full(&self) -> Arc<T> {
        self.current.load_full()
    }

    /// Return a value, so that reclaiming it will not happen on this thread.
    ///
    /// This should be the last reference to the `Arc` on a given thread.
    ///
    /// This is roughly waitfree if and only if it is called from one thread at a time.
    pub(crate) fn defer_reclaim(&self, update: Arc<T>) {
        let old_freelist_head = self.freelist.swap(None);
        if let Some(ref old) = old_freelist_head {
            // Only put this new update on the head of the freelist if the head is changing.
            if Arc::ptr_eq(old, &update) {
                return;
            }
        }

        // We own the Arc to push. Other threads should not.  We will RCU the value, because we know that even if the
        // head of the freelist changes, under proper usage, the above can never be true again.
        self.freelist.rcu(|_| Some(update.clone()));
    }

    fn reclaim_freelist(&self) {
        // The freelist may contain cycles. We will walk it breaking such cycles until we find an endpooint.
        let mut current = self.freelist.load_full();
        while let Some(n) = current {
            current = n.get_stash().next.swap(None);
        }
    }

    pub(crate) fn publish(&self, update: Arc<T>) {
        self.current.store(update);
    }
}

impl<T> ArcStash<T> {
    pub(crate) fn new() -> Self {
        Self {
            next: ArcSwapOption::default(),
        }
    }
}

impl<T> Default for ArcStash<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for ArcStash<T> {
    // Clones are new objects (e.g. not this one) ergo they should not bring along the freelist linkage.  Otherwise we
    // end up with a DAG.
    fn clone(&self) -> Self {
        Default::default()
    }
}

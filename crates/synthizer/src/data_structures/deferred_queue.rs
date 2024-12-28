//! A persistent queue which allows some thread to drain it without freeing the elements.
//!
//! This queue uses atomics for interior mutability, and a persistent queue for the items.  What happens here is that
//! the clone operation can also clear items, so they get cleared out on the user's threads, not in realtime contexts.
//!
//! See [DeferredQueue] for more.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use rpds::QueueSync as QS;

/// An MPSC queue of items, implemented as a persistent data structure.
///
/// Adding items to the queue does not make them appear at the other side.  Instead, they appear at the other side when
/// the queue is published through some mechanism.  For example, if they are part of the synthesizer state, they publish
/// on batch drop.  If more than one thread wishes to publish at a time, they must synchronize themselves, e.g. through
/// a mutex or RCU.  Otherwise items will be lost, since threads will roll each other's enqueueing back.
#[derive(Clone)]
pub(crate) struct DeferredQueue<T> {
    items: QS<Arc<QueueEntry<T>>>,
}

struct QueueEntry<T> {
    data: T,

    /// When this entry is consumed, this goes to true; that's the signal to let the producer know it should drop this.
    consumed: AtomicBool,
}

struct Guard<'a, T> {
    queue: &'a DeferredQueue<T>,
    item: &'a QueueEntry<T>,
}

impl<T: Send + Sync + 'static> DeferredQueue<T> {
    pub fn new() -> Self {
        Self {
            items: QS::new_sync(),
        }
    }

    /// Get an item.
    ///
    /// When the guard is dropped, the item will be freed by some producer in the future.
    fn dequeue(&self) -> Option<Guard<T>> {
        for i in self.items.iter() {
            if !i.consumed.load(Ordering::Acquire) {
                return Some(Guard {
                    queue: self,
                    item: i,
                });
            }
        }

        None
    }

    /// Publish an item to this queue.
    ///
    /// Generally this is called after a clone but, if a thread has a reference sufficient to let it do multiple mutable
    /// operations in a row, that's a big time savings.  rpds is good at letting us do this.
    pub(crate) fn push(&mut self, item: T) {
        self.producer_cleanup();

        let ent = Arc::new(QueueEntry {
            consumed: AtomicBool::new(false),
            data: item,
        });

        self.items.enqueue_mut(ent);
    }

    fn producer_cleanup(&mut self) {
        while let Some(q) = self.items.peek() {
            if q.consumed.load(Ordering::Relaxed) {
                self.items.dequeue_mut();
            } else {
                break;
            }
        }
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.item.consumed.store(true, Ordering::Release);
    }
}

impl<T> std::ops::Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.item.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_dequeue() {
        let mut queue = DeferredQueue::new();
        queue.push(1);
        queue.push(2);

        {
            let guard = queue.dequeue().unwrap();
            assert_eq!(*guard, 1);
        }

        {
            let guard = queue.dequeue().unwrap();
            assert_eq!(*guard, 2);
        }

        assert!(queue.dequeue().is_none());
    }

    #[test]
    fn test_producer_cleanup() {
        let mut queue = DeferredQueue::new();
        queue.push(1);
        queue.push(2);

        {
            let guard = queue.dequeue().unwrap();
            assert_eq!(*guard, 1);
        }

        queue.producer_cleanup();
        assert_eq!(queue.items.len(), 1);

        {
            let guard = queue.dequeue().unwrap();
            assert_eq!(*guard, 2);
        }

        queue.producer_cleanup();
        assert_eq!(queue.items.len(), 0);
    }
}

use std::any::TypeId;
use std::cmp::{Eq, Ord, PartialEq, PartialOrd};
use std::hash::Hash;
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A shared pointer.
///
/// This is like [std::sync::Arc], but can be backed by a custom allocator.  The allocator can vary per pointer.  These
/// are created through [crate::Allocator], which knows how to dispatch to different kinds of allocators based on
/// configuration.
pub struct SharedPtr<T: ?Sized + Send + Sync + 'static> {
    control_block: NonNull<SharedPtrControlBlock>,
    value: NonNull<T>,
}

/// A weak pointer.
///
/// this is like [std::sync::Weak], but for [SharedPtr]s.
pub struct WeakPtr<T: ?Sized + Send + Sync + 'static> {
    control_block: NonNull<SharedPtrControlBlock>,
    value: NonNull<T>,
}

mod sealed {
    use super::*;

    /// Kinds of operation we may request on a control block.
    pub(crate) enum ControlBlockOp {
        FreeControlBlock,
        FreeData,
    }

    /// A header for a reference-counted pointer.
    #[derive(Debug)]
    pub struct SharedPtrControlBlock {
        /// the original type stored in this pointer. Used to support safe downcasting.
        pub(crate) type_id: TypeId,

        /// The original data, of the most-derived type.
        pub(crate) original_data: NonNull<i8>,

        /// The callback to perform operations on this control block.
        ///
        /// Allocators that need more state wrap the control block in another struct and do pointer casting.
        pub(crate) control_callback: unsafe fn(NonNull<SharedPtrControlBlock>, ControlBlockOp),

        /// The reference count of the control block. Starts at 1.
        pub(crate) refcount: AtomicUsize,

        /// The reference count of the strong pointers. Also starts at 1.
        pub(crate) strong_refcount: AtomicUsize,
    }

    /// Trait representing something which can produce headers for shared pointers.
    pub trait SharedPtrAllocStrategy {
        fn do_alloc<T: Send + Sync + 'static>(
            &self,
            val: T,
        ) -> (NonNull<SharedPtrControlBlock>, NonNull<T>);
    }
}

pub(crate) use sealed::*;

impl SharedPtrControlBlock {
    /// Decrement the weak refcount  to the control block, and free it if necessary.
    ///
    /// Invalidates the pointer to the control block if the control block's refcount goes to 0.
    pub(crate) unsafe fn decref_weak(cb: NonNull<SharedPtrControlBlock>) {
        unsafe {
            let refcount = cb.as_ref().refcount.fetch_sub(1, Ordering::Release);
            if refcount == 1 {
                let callback = cb.as_ref().control_callback;
                callback(cb, ControlBlockOp::FreeControlBlock);
            }
        }
    }

    /// Decrement the strong refcount to the control block.
    ///
    /// Does not touch the weak reference count. Callers that need that should call both functions.
    ///
    /// If the strong refcount goes to 0, invalidates any pointer to the data.
    pub(crate) unsafe fn decref_strong(cb: NonNull<SharedPtrControlBlock>) {
        unsafe {
            let refcount = cb.as_ref().strong_refcount.fetch_sub(1, Ordering::Release);
            if refcount == 1 {
                let callback = cb.as_ref().control_callback;
                callback(cb, ControlBlockOp::FreeData);
            }
        }
    }
}

impl<T: Send + Sync + 'static> SharedPtr<T> {
    pub(crate) fn new<Alloc: SharedPtrAllocStrategy>(alloc: &Alloc, val: T) -> SharedPtr<T> {
        let (control_block, value) = alloc.do_alloc::<T>(val);
        unsafe {
            assert_eq!(control_block.as_ref().refcount.load(Ordering::Relaxed), 1);
            assert_eq!(
                control_block
                    .as_ref()
                    .strong_refcount
                    .load(Ordering::Relaxed),
                1
            );
        }

        SharedPtr {
            control_block,
            value,
        }
    }
}

impl<T: ?Sized + Send + Sync + 'static> Deref for SharedPtr<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.value.as_ref() }
    }
}

impl<T: ?Sized + Send + Sync + 'static> Drop for SharedPtr<T> {
    fn drop(&mut self) {
        unsafe {
            // Strong is always first.
            SharedPtrControlBlock::decref_strong(self.control_block);
            SharedPtrControlBlock::decref_weak(self.control_block);
        }
    }
}

impl<T: ?Sized + Send + Sync + 'static> WeakPtr<T> {
    pub fn new(strong: &SharedPtr<T>) -> Self {
        unsafe {
            let old = strong
                .control_block
                .as_ref()
                .refcount
                .fetch_add(1, Ordering::Relaxed);
            assert_ne!(old, 0);
        }

        WeakPtr {
            control_block: strong.control_block,
            value: strong.value,
        }
    }

    /// Upgrade this weak pointer to a strong pointer, if possible.
    pub fn upgrade(&self) -> Option<SharedPtr<T>> {
        // We can only upgrade if it is possible to increase the strong reference count from 0, which is *not* the same
        // as just trying to increment it and checking.  Incrementing and checking "resurrects" the object, which is
        // invalid in the case of a 0 refcount.
        unsafe {
            let mut cur = self
                .control_block
                .as_ref()
                .strong_refcount
                .load(Ordering::Relaxed);
            loop {
                if cur == 0 {
                    return None;
                }
                match self
                    .control_block
                    .as_ref()
                    .strong_refcount
                    .compare_exchange(cur, cur + 1, Ordering::Acquire, Ordering::Relaxed)
                {
                    Err(x) => {
                        cur = x;
                    }
                    Ok(_) => {
                        self.control_block
                            .as_ref()
                            .refcount
                            .fetch_add(1, Ordering::Acquire);
                        return Some(SharedPtr {
                            control_block: self.control_block,
                            value: self.value,
                        });
                    }
                }
            }
        }
    }
}

impl<T: ?Sized + Send + Sync + 'static> Drop for WeakPtr<T> {
    fn drop(&mut self) {
        unsafe {
            SharedPtrControlBlock::decref_weak(self.control_block);
        }
    }
}

impl<T: ?Sized + Send + Sync + 'static> Clone for SharedPtr<T> {
    fn clone(&self) -> Self {
        let old_cb_ref = unsafe {
            self.control_block
                .as_ref()
                .refcount
                .fetch_add(1, Ordering::Relaxed)
        };
        let old_strong_ref = unsafe {
            self.control_block
                .as_ref()
                .strong_refcount
                .fetch_add(1, Ordering::Relaxed)
        };
        assert_ne!(old_cb_ref, 0);
        assert_ne!(old_strong_ref, 0);
        SharedPtr {
            control_block: self.control_block,
            value: self.value,
        }
    }
}

// Implement all our annoying traits that pass through.

impl<T: ?Sized + Send + Sync + 'static + PartialEq> PartialEq for SharedPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        let left: &T = self.deref();
        let right: &T = other.deref();
        left.eq(right)
    }
}

impl<T: ?Sized + Send + Sync + 'static + Eq> Eq for SharedPtr<T> {}

impl<T: ?Sized + Send + Sync + 'static + PartialOrd> PartialOrd for SharedPtr<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let left: &T = self.deref();
        let right: &T = other.deref();
        left.partial_cmp(right)
    }
}

impl<T: ?Sized + Send + Sync + 'static + Ord> Ord for SharedPtr<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let left: &T = self.deref();
        let right: &T = other.deref();
        left.cmp(right)
    }
}

impl<T: ?Sized + Send + Sync + 'static + Hash> Hash for SharedPtr<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let inner: &T = self.deref();
        inner.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::allocation_strategies::SimpleBoxStrategy;

    /// These tests get tricky because any method that lets us track drops will introduce additional synchronization. We
    /// instead use a struct which sets its single u64 field to a garbage value when dropped so that it will be obvious
    /// if e.g. we reallocate over something old without initialization.  This isn't great, but it's the best we can do.
    /// Fortunately, this is one of the few places with significantly complex unsafe, and the higher level pieces get to
    /// avoid the issue.
    struct GarbageDropper(u64);

    impl Drop for GarbageDropper {
        fn drop(&mut self) {
            self.0 = u64::MAX;
        }
    }

    #[test]
    fn test_simple() {
        let alloc = SimpleBoxStrategy;

        let ptr = SharedPtr::new(&alloc, GarbageDropper(1));
        assert_eq!(ptr.0, 1);
        let weak = WeakPtr::new(&ptr);
        assert_eq!(weak.upgrade().unwrap().0, 1);
        std::mem::drop(ptr);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn test_cloning() {
        let alloc = SimpleBoxStrategy;

        let ptr = SharedPtr::new(&alloc, GarbageDropper(1));
        let ptr2 = ptr.clone();

        assert_eq!(ptr.0, 1);
        assert_eq!(ptr2.0, 1);
        assert_eq!(
            unsafe { ptr.control_block.as_ref().refcount.load(Ordering::Relaxed) },
            2
        );
        assert_eq!(
            unsafe {
                ptr.control_block
                    .as_ref()
                    .strong_refcount
                    .load(Ordering::Relaxed)
            },
            2
        );

        std::mem::drop(ptr);
        assert_eq!(
            unsafe { ptr2.control_block.as_ref().refcount.load(Ordering::Relaxed) },
            1
        );

        assert_eq!(
            unsafe {
                ptr2.control_block
                    .as_ref()
                    .strong_refcount
                    .load(Ordering::Relaxed)
            },
            1
        );

        let weak = WeakPtr::new(&ptr2);
        assert_eq!(
            unsafe { ptr2.control_block.as_ref().refcount.load(Ordering::Relaxed) },
            2
        );
        assert_eq!(
            unsafe {
                ptr2.control_block
                    .as_ref()
                    .strong_refcount
                    .load(Ordering::Relaxed)
            },
            1
        );

        assert!(weak.upgrade().is_some());

        std::mem::drop(ptr2);
        assert!(weak.upgrade().is_none());

        assert_eq!(
            unsafe { weak.control_block.as_ref().refcount.load(Ordering::Relaxed) },
            1
        );
        assert_eq!(
            unsafe {
                weak.control_block
                    .as_ref()
                    .strong_refcount
                    .load(Ordering::Relaxed)
            },
            0
        );
    }

    struct CloseDrop(crossbeam::channel::Sender<()>);

    #[test]
    fn test_dropping() {
        let (sender, receiver) = crossbeam::channel::bounded(1);
        let alloc = SimpleBoxStrategy;

        let ptr = SharedPtr::new(&alloc, CloseDrop(sender));
        #[allow(clippy::redundant_clone)]
        let _ = ptr.clone();
        std::mem::drop(ptr);

        assert!(receiver.recv().is_err());
    }
    // Sadly there is essentially no useful multithreaded test here, so that's about all we can do for now.
}

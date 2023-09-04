use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::num::NonZeroU16;
use std::ptr::NonNull;

use crate::generational_atomic::GenerationalAtomicU32;
use crate::optional_atomic_u32::OptionalAtomicU32;
use crate::sync::{atomic::Ordering, Arc};

/// A pool of fixed sized backed by an MPMC stack.
///
/// The underlying pool may contain up to `u32::MAX - 1` items.
pub struct FixedSizePool<T> {
    /// 0 is None, all other heads are `1 + head`.
    head: GenerationalAtomicU32,
    elements: Vec<MaybeUninit<UnsafeCell<T>>>,
    pointers: Vec<OptionalAtomicU32>,
}

/// A reference into a [FixedSizePool].
///
/// Dropping this reference will free the element in the pool.
pub struct ExclusiveFixedSizePoolHandle<T: Send + Sync> {
    pool: Arc<FixedSizePool<T>>,
    index: u16,
    data: NonNull<T>,
}

impl<T: Send + Sync> FixedSizePool<T> {
    /// Create a pool of a fixed size.
    ///
    /// capacity must never be `u16::MAX`.
    pub fn new(capacity: NonZeroU16) -> Self {
        // Really we could do usize, and right now we're using 32-bit atomics for the pointers, but this can all be
        // optimized later so we save the flexibility for optimization.
        assert_ne!(capacity.get(), u16::MAX);

        let mut ret = Self {
            head: GenerationalAtomicU32::new(0),
            elements: vec![],
            pointers: vec![],
        };

        // This is safe: UnsafeCell<MaybeUninit<...>>.
        ret.elements.reserve(capacity.get() as usize);
        unsafe {
            ret.elements.set_len(capacity.get() as usize);
        }

        // The pointers each point at the next lowest element up to the last, and then the head is initialized to the
        // last.
        ret.pointers.reserve(capacity.get() as usize);
        ret.pointers.push(OptionalAtomicU32::new(None));
        for i in 1..capacity.get() as usize {
            ret.pointers
                .push(OptionalAtomicU32::new(Some(i as u32 - 1)));
        }

        // Remember that head of 0 is NULL and head starts at 1; there is no - 1 here for that reason.
        ret.head
            .store_slow(capacity.get() as u32, Ordering::Relaxed);

        ret
    }

    /// Allocate an index from the pool.
    fn alloc_index(&self) -> Option<u16> {
        let mut head = self.head.load(Ordering::Relaxed);
        loop {
            let head_ind = head.get().checked_sub(1)?;
            let new_head = self.pointers[head_ind as usize]
                .load(Ordering::Relaxed)
                .map(|x| x + 1)
                .unwrap_or(0);
            match self
                .head
                .compare_exchange(head, new_head, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(_) => return Some(head_ind.try_into().unwrap()),
                Err(e) => head = e,
            }
        }
    }

    /// Return an index to the stack.
    fn free_index(&self, index: u16) {
        let mut head = self.head.load(Ordering::Relaxed);
        loop {
            let new_next = head.get().checked_sub(1);
            // We own this entry.
            self.pointers[index as usize].store(new_next, Ordering::Relaxed);
            match self.head.compare_exchange(
                head,
                (index as u32) + 1,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(e) => head = e,
            }
        }
    }

    /// Allocate an entry in the pool if possible, otherwise return `Err` with the passed-in value.
    #[cfg(not(loom))]
    pub fn allocate(self: &Arc<Self>, new_val: T) -> Result<ExclusiveFixedSizePoolHandle<T>, T> {
        let Some(index) = self.alloc_index() else {
            return Err(new_val);
        };

        let ptr = {
            // fine: immutable ref to immutable ref.
            let r = &self.elements[index as usize];
            // Fine: UnsafeCell::raw_get over uninitialized memory is defined as okay via STD docs.
            UnsafeCell::raw_get(r.as_ptr())
        };

        // ptr is uninitialized, so:
        unsafe { ptr.write(new_val) };

        Ok(ExclusiveFixedSizePoolHandle {
            pool: self.clone(),
            index,
            data: NonNull::new(ptr).unwrap(),
        })
    }
}

impl<T: Send + Sync> std::ops::Deref for ExclusiveFixedSizePoolHandle<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.data.as_ref() }
    }
}

impl<T: Send + Sync> std::ops::DerefMut for ExclusiveFixedSizePoolHandle<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.data.as_mut() }
    }
}

unsafe impl<T: Send + Sync> Send for FixedSizePool<T> {}
unsafe impl<T: Send + Sync> Sync for FixedSizePool<T> {}
unsafe impl<T: Send + Sync> Send for ExclusiveFixedSizePoolHandle<T> {}
unsafe impl<T: Send + Sync> Sync for ExclusiveFixedSizePoolHandle<T> {}

impl<T: Send + Sync> Drop for ExclusiveFixedSizePoolHandle<T> {
    fn drop(&mut self) {
        unsafe { self.data.as_ptr().drop_in_place() };
        self.pool.free_index(self.index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that we can allocate a set of unique indices across multiple threads, and that this will always use all of
    /// the pool.
    #[test]
    fn pushing_always_unique() {
        crate::sync::wrap_test(|| {
            let pool = Arc::new(FixedSizePool::<u32>::new(NonZeroU16::new(9).unwrap()));

            let mut thread_handles = vec![];

            for _ in 0..3 {
                let pool = pool.clone();
                thread_handles.push(crate::sync::spawn(move || {
                    let mut res = vec![];
                    for _ in 0..3 {
                        res.push(pool.alloc_index().unwrap());
                    }
                    res
                }));
            }

            let mut results = thread_handles
                .into_iter()
                .flat_map(|x| x.join().unwrap().into_iter())
                .collect::<Vec<u16>>();
            results.sort_unstable();
            assert_eq!(results, vec![0, 1, 2, 3, 4, 5, 6, 7, 8,],);
        });
    }

    /// This test tries to catch ABA problems by popping and pushing indices.  It ensurs that there are never duplicate
    /// indices alive at any point, and additionally that no indices are lost.
    #[test]
    fn fuzz_aba() {
        use std::sync::Arc as StdArc;
        use std::sync::Mutex as StdMutex;

        enum Op {
            // an alloc_index call returned this value.
            Alloc(u16),

            // A free_index call was called with this value.
            Free(u16),
        }

        crate::sync::wrap_test(|| {
            // When using Loom, this mutex is basically RefCell.  When not using Loom, it's a normal mutex.  In either
            // case, we don't want it to be visible to Loom because it doesn't contribute to the thread ordering, and is
            // simply used to collect the sequence of what happened for validation.
            let ops: StdArc<StdMutex<Vec<Op>>> = StdArc::new(StdMutex::new(vec![]));

            // Note in the loop below that we will allocate at most 1 + 2 + 3 = 6 items. If that's not possible, we
            // found a situation in which there's some sort of ABA problem; any sequence of 6 alloc calls with a
            // capacity 6 pool should succeed, as long as there are exactly 6, even with returns between.
            let pool = Arc::new(FixedSizePool::<u32>::new(NonZeroU16::new(6).unwrap()));

            // alloc_bound is how many indices each thread is going to try to grab.
            for alloc_bound in 1..=3 {
                let pool = pool.clone();
                let ops = ops.clone();

                crate::sync::spawn(move || {
                    let mut local_indices = vec![];

                    for _ in 0..alloc_bound {
                        let index = pool.alloc_index().unwrap();
                        ops.lock().unwrap().push(Op::Alloc(index));
                        local_indices.push(index);
                    }

                    for ind in local_indices {
                        pool.free_index(ind);
                        ops.lock().unwrap().push(Op::Free(ind));
                    }
                });
            }

            let mut alive: std::collections::HashSet<u16> = Default::default();
            let mut expected_alive_count = 0;

            let ops_guard = ops.lock().unwrap();

            for o in ops_guard.iter() {
                match o {
                    Op::Alloc(x) => {
                        assert!(*x < 6);
                        alive.insert(*x);
                        expected_alive_count += 1;
                    }
                    Op::Free(x) => {
                        assert!(alive.remove(x));
                        expected_alive_count -= 1;
                    }
                }

                assert_eq!(alive.len(), expected_alive_count);
            }
        });
    }

    #[cfg(not(loom))]
    #[test]
    fn test_dropping() {
        let dropper = eye_dropper::EyeDropper::<u32>::new();

        let pool = Arc::new(FixedSizePool::new(NonZeroU16::new(3).unwrap()));

        let (l1, t1) = dropper.new_value(1);
        let (l2, t2) = dropper.new_value(2);
        let (l3, t3) = dropper.new_value(3);

        let handles = [t1, t2, t3]
            .into_iter()
            .map(|x| pool.allocate(x).unwrap())
            .collect::<Vec<_>>();

        l1.assert_alive();
        l2.assert_alive();
        l3.assert_alive();
        dropper.assert_exact(0);

        std::mem::drop(handles);
        l1.assert_dropped();
        l2.assert_dropped();
        l3.assert_dropped();
        dropper.assert_exact(3);
    }

    /// if we grab exactly as many handles and drop them and grab them and... a few times, do we invalidate the data
    /// structure?
    ///
    /// This is a lesser version of the loom tests which can tell us that the single-threaded case is right, thus
    /// providing more confidence in CI (currently, we don't have enough CI resources to always run loom).
    #[cfg(not(loom))]
    #[test]
    fn test_spin_alloc() {
        const CAP: u16 = 10;
        const ATTEMPTS: usize = 100;

        let pool = Arc::new(FixedSizePool::<u32>::new(NonZeroU16::new(CAP).unwrap()));

        let mut handles = vec![];

        for _ in 0..ATTEMPTS {
            for _ in 0..CAP {
                handles.push(pool.allocate(1).unwrap());
            }

            handles.clear();
        }
    }

    #[cfg(not(loom))]
    #[test]
    fn handles_are_distinct_when_writing() {
        const CAP: u16 = 10;
        let pool = Arc::new(FixedSizePool::new(NonZeroU16::new(CAP).unwrap()));

        let mut handles = (0..CAP)
            .map(|x| pool.allocate(x).unwrap())
            .collect::<Vec<_>>();
        for h in handles.iter_mut() {
            **h *= 2;
        }

        // If the locations were distinct, then doubling all these values produces a specific vec, as below.
        let got = handles.into_iter().map(|x| *x).collect::<Vec<_>>();
        assert_eq!(got, vec![0, 2, 4, 6, 8, 10, 12, 14, 16, 18]);
    }

    #[cfg(not(loom))]
    #[test]
    fn pool_stops_when_full() {
        const CAP: u16 = 10;
        let pool = Arc::new(FixedSizePool::<u16>::new(NonZeroU16::new(CAP).unwrap()));

        // This exists to keep the handles alive.
        let _allowed_handles = (0..CAP)
            .map(|x| pool.allocate(x).unwrap())
            .collect::<Vec<_>>();

        assert!(pool.allocate(100).is_err());
    }
}

use std::alloc::Layout;
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

use crate::generational_atomic::GenerationalAtomicU32;
use crate::optional_atomic_u32::OptionalAtomicU32;
use crate::sync::Ordering;
use crate::sync::{Arc, RwLock};

/// A root of an allocation, containing the layout and a pointer to be freed via the global allocator.
struct AllocationRoot {
    layout: Layout,
    data: NonNull<u8>,
}

impl AllocationRoot {
    fn alloc_for<T>(elements: usize) -> Self {
        assert!(elements > 0);
        let layout = Layout::array::<T>(elements).unwrap();
        if layout.size() == 0 {
            AllocationRoot {
                layout,
                data: NonNull::dangling(),
            }
        } else {
            let data = unsafe { std::alloc::alloc(layout) };
            AllocationRoot {
                layout,
                data: NonNull::new(data).unwrap(),
            }
        }
    }
}

unsafe impl Send for AllocationRoot {}

impl Drop for AllocationRoot {
    fn drop(&mut self) {
        if self.layout.size() > 0 {
            unsafe {
                std::alloc::dealloc(self.data.as_ptr(), self.layout);
            }
        }
    }
}

struct SlabElement<T> {
    /// The next free element, if this element is on the freelist.
    next_free: OptionalAtomicU32,

    /// Guarded by the exclusive handle. Only ever accessed in creation, destruction, or via deref impls on handles.
    data: MaybeUninit<UnsafeCell<T>>,
}

// These impls are necessary so that the slab overall is sync from the external user's perspective.
unsafe impl<T: Send> Send for SlabElement<T> {}
unsafe impl<T: Sync> Sync for SlabElement<T> {}

/// The internal state of a slab consists of two vectors:
///
/// - roots contains the roots of the pages, and exists only so that things free on drop.
/// - elements contains the actual data, but as pointers which we dereference so that the vec can grow safely.
struct SlabVecs<T> {
    // Used for dropping only.
    #[allow(dead_code)]
    roots: Vec<AllocationRoot>,
    elements: Vec<NonNull<SlabElement<T>>>,
}

unsafe impl<T: Send> Send for SlabVecs<T> {}
unsafe impl<T: Sync> Sync for SlabVecs<T> {}

/// Internal state of a slab. External users actually get an arc to the whole thing.
struct SlabState<T> {
    inner: RwLock<SlabVecs<T>>,

    /// 0 means None, and all indices are -1 from nonzero values.
    freelist_head: GenerationalAtomicU32,
}

impl<T> SlabState<T> {
    pub fn new(initial_capacity: u32) -> SlabState<T> {
        let inner = SlabVecs {
            elements: Default::default(),
            roots: Default::default(),
        };

        let state = SlabState {
            inner: RwLock::new(inner),
            freelist_head: GenerationalAtomicU32::new(0),
        };

        state.grow(|_| initial_capacity);

        unsafe {
            assert!(state
                .inner
                .read()
                .unwrap()
                .elements
                .get(0)
                .unwrap()
                .as_ref()
                .next_free
                .load(Ordering::Relaxed)
                .is_some());
        }
        state
    }

    /// Grow the slab to the given new size.  The provided closure gets to manipulate the current size, e.g.
    /// multiplication, etc.
    ///
    /// Holds the inner rwlock.
    fn grow(&self, new_size_fn: impl FnOnce(u32) -> u32) {
        let mut guard = self.inner.write().unwrap();
        let now = guard.elements.len() as u32;
        let new_size = new_size_fn(now);
        assert!(new_size > now);
        let delta = new_size - now;
        let new_root = AllocationRoot::alloc_for::<SlabElement<T>>(delta as usize);

        for i in 0..delta {
            unsafe {
                let p = new_root
                    .data
                    .as_ptr()
                    .cast::<SlabElement<T>>()
                    .add(i as usize);
                std::ptr::write(
                    p,
                    SlabElement {
                        // We will link things so that the lowest element is the head of the list, and each element is
                        // linked to the one after.  This lets us be somewhat cache local.
                        //
                        // be careful. We need +1.  Consider what happens if i == 0.  In that case, the element which is
                        // taking index now points at now, and so on.
                        next_free: OptionalAtomicU32::new(Some(now + i + 1)),
                        data: MaybeUninit::uninit(),
                    },
                );
                guard.elements.push(NonNull::new(p).unwrap());
            }
        }

        // The last element is wrong, and attempts to link to an element that doesn't exist; what we actually want
        // is to link it to the current head of the freelist.
        //
        // The head of the freelist is currently not being manipulated by anyone else, since we hold the write side
        // of the mutex.
        let old_head = self.freelist_head.load(Ordering::Relaxed).get();
        let val = old_head.checked_sub(1); // goes to None if this was already 0.
        unsafe {
            guard
                .elements
                .last()
                .unwrap()
                .as_ref()
                .next_free
                .store(val, Ordering::Relaxed);
        }

        // Don't forget that 0 means none, so this is +1.
        let new_head: u32 = now.checked_add(1).unwrap();
        self.freelist_head.store_slow(new_head, Ordering::Relaxed);
        guard.roots.push(new_root);
    }

    /// Allocate an index, and return both that index and a pointer to the underlying data.
    fn alloc_index(&self) -> (u32, NonNull<SlabElement<T>>) {
        loop {
            // First we try to do this via the read side.
            {
                let guard = self.inner.read().unwrap();

                // We use 0 to mean none.
                let mut possible_next = self.freelist_head.load(Ordering::Acquire);
                while possible_next.get() != 0 {
                    let ind = (possible_next.get() - 1) as usize;
                    let new_head = unsafe {
                        guard.elements[ind]
                            .as_ref()
                            .next_free
                            .load(Ordering::Relaxed)
                    };
                    let new_head = new_head.map(|x| x + 1).unwrap_or(0);
                    match self.freelist_head.compare_exchange(
                        possible_next,
                        new_head,
                        Ordering::Release,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => {
                            return (ind.try_into().unwrap(), guard.elements[ind]);
                        }
                        Err(x) => {
                            possible_next = x;
                            crate::sync::yield_if_loom();
                        }
                    }
                }
            }

            // Otherwise, grow and try again.
            self.grow(|x| {
                let nx = x.saturating_mul(2);
                assert!(nx > x);
                nx
            });
        }
    }

    /// Free an index by reinserting it onto the freelist.
    fn free_index(&self, index: u32) {
        let guard = self.inner.read().unwrap();
        let elem = guard.elements.get(index as usize).unwrap();

        let mut head = self.freelist_head.load(Ordering::Relaxed);
        loop {
            unsafe {
                elem.as_ref()
                    .next_free
                    .store(head.get().checked_sub(1), Ordering::Relaxed)
            };
            match self.freelist_head.compare_exchange(
                head,
                index.checked_add(1).unwrap(),
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => {
                    head = x;
                    crate::sync::yield_if_loom();
                }
            }
        }
    }
}

/// A handle to a slab which never enters the kernel unless reallocating.
/// 
/// Allocation and freeing from one thread is O(1).  Allocation and freeing from N threads spins, if and only if they
/// all try to touch the same atomic u32 at the same time.  in other words, primarily single threaded use or use at low
/// throughput will never block, unless the capacity is exceeded.
/// 
/// Unlike other data structures in this crate, it isn't possible to build something fully wait-free because
/// reallocation becomes a problem.  Note that the internal implementation currently uses std's RwLock.  Unless
/// reallocating, only the read side is acquired.  This is good enough for now, but will be moved to a spinlock later.
pub struct SlabHandle<T> {
    state: Arc<SlabState<T>>,
}

pub struct ExclusiveSlabRef<T> {
    data: NonNull<SlabElement<T>>,
    index: u32,
    slab: Arc<SlabState<T>>,
}

impl<T: Send> SlabHandle<T> {
    pub fn new(initial_capacity: u32) -> SlabHandle<T> {
        SlabHandle {
            state: Arc::new(SlabState::new(initial_capacity)),
        }
    }

    pub fn insert(&self, val: T) -> ExclusiveSlabRef<T> {
        let (index, mut data) = self.state.alloc_index();
        unsafe {
            data.as_mut().data.write(UnsafeCell::new(val));
        }

        ExclusiveSlabRef {
            index,
            data,
            slab: self.state.clone(),
        }
    }
}

unsafe impl<T: Send> Send for ExclusiveSlabRef<T> {}
unsafe impl<T: Sync> Sync for ExclusiveSlabRef<T> {}

impl<T> Clone for SlabHandle<T> {
    fn clone(&self) -> Self {
        SlabHandle {
            state: self.state.clone(),
        }
    }
}

impl<T> std::ops::Deref for ExclusiveSlabRef<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe {
            self.data
                .as_ref()
                .data
                .as_ptr()
                .as_ref()
                .unwrap_unchecked()
                .get()
                .as_ref()
                .unwrap_unchecked()
        }
    }
}

impl<T> std::ops::DerefMut for ExclusiveSlabRef<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            self.data
                .as_mut()
                .data
                .as_mut_ptr()
                .as_mut()
                .unwrap_unchecked()
                .get_mut()
        }
    }
}

impl<T> Drop for ExclusiveSlabRef<T> {
    fn drop(&mut self) {
        unsafe {
            self.data.as_mut().data.assume_init_drop();
        }
        self.slab.free_index(self.index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use eye_dropper::{EyeDropper, TrackedDrop};

    use std::collections::HashSet;

    // This model is too complex for loom, so we must suffer.
    #[cfg(not(loom))]
    #[test]
    fn test_concurrent_slab() {
        const THREADS: u32 = 500;
        const NUM_PER_THREAD: u32 = 15;

        let slab = SlabHandle::<TrackedDrop<u32>>::new(2);

        let mut join_handles: Vec<crate::sync::JoinHandle<()>> = vec![];

        // This is intentionally invisible to loom. Tracks the currently allocated indices.
        let allocated_indices: Arc<std::sync::Mutex<HashSet<u32>>> =
            Arc::new(std::sync::Mutex::new(Default::default()));

        for i in 0..THREADS {
            let slab = slab.clone();
            let allocated_indices = allocated_indices.clone();

            let jh = crate::sync::spawn(move || {
                let base = i * 5;
                let handle_source = EyeDropper::new();

                // First, grab two values. Their indices should be different, and storing to them should work.
                let (l1, t1) = handle_source.new_value(base);
                let (l2, t2) = handle_source.new_value(base + 1);

                let mut v1 = slab.insert(t1);
                let mut v2 = slab.insert(t2);
                assert!(v1.data != v2.data, "{} {}", v1.index, v2.index);
                assert!(v1.index != v2.index);

                **v1 = 100 + 100 * i;
                **v2 = 200 + 200 * i;

                assert_eq!(**v1, 100 + 100 * i);
                assert_eq!(**v2, 200 + 200 * i);

                std::mem::drop(v1);
                std::mem::drop(v2);

                l1.assert_dropped();
                l2.assert_dropped();

                let mut vals: Vec<ExclusiveSlabRef<TrackedDrop<u32>>> = vec![];

                // Using a closure here makes the borrow checker sad.
                macro_rules! clear_vec {
                    () => {
                        let mut guard = allocated_indices.lock().unwrap();
                        for v in vals.iter() {
                            assert!(guard.remove(&v.index));
                        }
                        vals.clear();
                    };
                }

                // For our next test, we allocate and immediately drop fixed numbers of elements.  We should never
                // see the same allocated index more than once.
                for before_drop in 1..=5 {
                    for attempt in 0..NUM_PER_THREAD {
                        if attempt % before_drop == 0 {
                            clear_vec!();
                        }

                        let (_, t1) = handle_source.new_value(base + attempt);
                        let v1 = slab.insert(t1);
                        let mut guard = allocated_indices.lock().unwrap();
                        assert!(guard.insert(v1.index));
                        vals.push(v1);
                    }

                    clear_vec!();
                }
            });
            join_handles.push(jh);
        }

        for h in join_handles {
            h.join().unwrap();
        }

        // If the slab ever grows beyond this, then something is very wrong and we are leaking.
        let slab_len = slab.state.inner.read().unwrap().elements.len();
        let expected = ((THREADS * NUM_PER_THREAD) as usize).next_power_of_two();
        assert!(
            slab_len <= expected,
            "slab_len={}, expected={}",
            slab_len,
            expected
        );
    }
}

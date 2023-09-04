use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::fixed_size_pool::*;
use crate::prepend_only_list::PrependOnlyList;

struct SlabState<T: Send + Sync> {
    pool_list: PrependOnlyList<FixedSizePool<T>>,

    /// When this goes to true, attempts to allocate items will spin.
    ///
    /// This is effectively a mutex, but without touching the kernel.
    is_allocating: AtomicBool,

    /// Only ever touched when is_allocating is true, and consequently owned by only one thread at a time.
    next_capacity: UnsafeCell<u16>,
}

unsafe impl<T: Send + Sync> Send for SlabState<T> {}
unsafe impl<T: Send + Sync> Sync for SlabState<T> {}

/// A handle to a slab which never enters the kernel unless reallocating to grow.
pub struct SlabHandle<T: Send + Sync> {
    state: Arc<SlabState<T>>,
}

pub struct ExclusiveSlabRef<T: Send + Sync> {
    data: ExclusiveFixedSizePoolHandle<T>,
}

const POOL_CAP_LIMIT: usize = (u16::MAX - 1) as usize;

fn next_cap(cap: u16) -> u16 {
    cap.max(1).saturating_mul(2).min(POOL_CAP_LIMIT as u16)
}

/// Returns (full_iterations, partial_value) given an initial capacity.
fn breakdown_initial_cap(cap: usize) -> (usize, usize) {
    let pools = cap / POOL_CAP_LIMIT;
    let left = cap % POOL_CAP_LIMIT;
    (pools, left)
}
impl<T: Send + Sync> SlabState<T> {
    fn new(initial_capacity: usize) -> Arc<Self> {
        let next_cap = initial_capacity.min(POOL_CAP_LIMIT);

        let (iters, rem) = breakdown_initial_cap(initial_capacity);

        let pool_list = PrependOnlyList::new();

        if rem != 0 {
            let rem_pool = Arc::new(FixedSizePool::new(
                std::num::NonZeroU16::new(rem as u16).unwrap(),
            ));
            pool_list.prepend(rem_pool);
        }

        for _ in 0..iters {
            let pool = Arc::new(FixedSizePool::new(
                std::num::NonZeroU16::new(POOL_CAP_LIMIT as u16).unwrap(),
            ));
            pool_list.prepend(pool);
        }

        Arc::new(SlabState {
            pool_list,
            is_allocating: AtomicBool::new(false),
            next_capacity: UnsafeCell::new(next_cap.try_into().unwrap()),
        })
    }

    fn try_grow(&self) {
        if self
            .is_allocating
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // We didn't get the lock.
            return;
        }

        let cap = unsafe { self.next_capacity.get().read() };
        let new_pool = Arc::new(FixedSizePool::new(std::num::NonZeroU16::new(cap).unwrap()));
        self.pool_list.prepend(new_pool);
        let new_cap = next_cap(cap);
        unsafe { self.next_capacity.get().write(new_cap) };

        self.is_allocating.store(false, Ordering::Release);
    }

    fn insert(&self, mut val: T) -> ExclusiveSlabRef<T> {
        loop {
            for i in self.pool_list.iter() {
                match i.allocate(val) {
                    Ok(data) => return ExclusiveSlabRef { data },
                    Err(v) => val = v,
                }
            }

            self.try_grow();
        }
    }
}

impl<T: Send + Sync> SlabHandle<T> {
    pub fn new(initial_capacity: usize) -> SlabHandle<T> {
        Self {
            state: SlabState::new(initial_capacity),
        }
    }

    pub fn insert(&self, val: T) -> ExclusiveSlabRef<T> {
        self.state.insert(val)
    }
}

impl<T: Send + Sync> Clone for SlabHandle<T> {
    fn clone(&self) -> Self {
        SlabHandle {
            state: self.state.clone(),
        }
    }
}

impl<T: Send + Sync> std::ops::Deref for ExclusiveSlabRef<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T: Send + Sync> std::ops::DerefMut for ExclusiveSlabRef<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;

    #[test]
    fn basic_allocation() {
        let handle = SlabHandle::<u32>::new(1);

        let got = (0..10).map(|x| handle.insert(x)).collect::<Vec<_>>();
        let got = got.into_iter().map(|x| *x).collect::<Vec<u32>>();

        assert_eq!(got, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_breakdown_initial_cap() {
        assert_eq!(breakdown_initial_cap(100), (0, 100));
        assert_eq!(breakdown_initial_cap((u16::MAX - 1) as usize), (1, 0));
        assert_eq!(breakdown_initial_cap(u16::MAX as usize), (1, 1));
        assert_eq!(breakdown_initial_cap(10000000), (152, 38832));
    }

    #[test]
    fn test_next_cap() {
        assert_eq!(next_cap(0), 2);
        assert_eq!(next_cap(100), 200);
        assert_eq!(next_cap(60000), u16::MAX - 1);
        assert_eq!(next_cap(u16::MAX), u16::MAX - 1);
    }
}

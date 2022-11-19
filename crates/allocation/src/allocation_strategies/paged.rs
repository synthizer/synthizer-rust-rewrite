use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};

use crate::allocation_page::AllocationPage;
use crate::shared_ptr::*;

#[repr(C)]
struct PagedControlBlock {
    /// Must always be first.
    base: SharedPtrControlBlock,

    /// The page is always protected by an arc.  We can have arcs inside the page, as long as we are careful to clone
    /// the arc before freeing the control block, so that we guarantee the page is valid.
    page: Arc<dyn Any + Send + Sync + 'static>,

    /// Always Some by the time the control block is allocated.
    control_page: Option<Arc<AllocationPage<PagedControlBlock>>>,
}

/// A strategy which fulfills requests by allocating pages as needed.
pub(crate) struct PagedStrategy {
    control_block_page: RwLock<Arc<AllocationPage<PagedControlBlock>>>,

    #[allow(clippy::type_complexity)]
    pages: RwLock<
        HashMap<TypeId, Arc<RwLock<Arc<dyn Any + Send + Sync + 'static>>>, ahash::RandomState>,
    >,

    /// the page size in elements.
    page_elements: usize,

    /// Number of control blocks per control block page.
    control_block_elements: usize,
}

impl PagedStrategy {
    pub(crate) fn new(control_block_elements: usize, page_elements: usize) -> Self {
        Self {
            control_block_page: RwLock::new(Arc::new(AllocationPage::new(control_block_elements))),
            pages: Default::default(),
            page_elements,
            control_block_elements,
        }
    }

    fn alloc_control_block<T: 'static>(
        &self,
        original_data: NonNull<T>,
        for_page: Arc<AllocationPage<T>>,
    ) -> NonNull<PagedControlBlock> {
        let mut block = PagedControlBlock {
            base: SharedPtrControlBlock {
                control_callback: paged_control_block_op::<T>,
                type_id: TypeId::of::<T>(),
                original_data: original_data.cast(),
                refcount: AtomicUsize::new(1),
                strong_refcount: AtomicUsize::new(1),
            },
            page: for_page,
            control_page: None,
        };

        {
            let guard = self.control_block_page.read().unwrap();
            match guard.allocate(block) {
                Ok(mut p) => {
                    unsafe { p.as_mut().control_page = Some(guard.clone()) };
                    return p;
                }
                Err(b) => block = b,
            }
        }

        // Unfortunately we will need a new block. Lock, check one more time, then make a new one if we have to.
        let mut guard = self.control_block_page.write().unwrap();

        for _ in 0..2 {
            match guard.allocate(block) {
                Ok(mut p) => {
                    unsafe {
                        p.as_mut().control_page = Some(guard.clone());
                    }
                    return p;
                }
                Err(b) => block = b,
            }

            *guard = Arc::new(AllocationPage::new(self.control_block_elements));
        }

        unreachable!();
    }

    fn upsert_page_lock<T: 'static>(&self) -> Arc<RwLock<Arc<dyn Any + Send + Sync + 'static>>> {
        let tid = TypeId::of::<T>();
        {
            let guard = self.pages.read().unwrap();
            if let Some(e) = guard.get(&tid) {
                return e.clone();
            }
        }

        let mut guard = self.pages.write().unwrap();
        guard
            .entry(tid)
            .or_insert_with(|| {
                Arc::new(RwLock::new(Arc::new(AllocationPage::<T>::new(
                    self.page_elements,
                ))))
            })
            .clone()
    }

    fn alloc_data<T: Send + Sync + 'static>(
        &self,
        mut val: T,
    ) -> (Arc<AllocationPage<T>>, NonNull<T>) {
        let lock = self.upsert_page_lock::<T>();

        {
            let guard = lock.read().unwrap();
            let page: Arc<AllocationPage<T>> =
                Arc::downcast::<AllocationPage<T>>(guard.clone()).unwrap();
            match page.allocate(val) {
                Ok(v) => {
                    return (page, v);
                }
                Err(v) => val = v,
            }
        }

        let mut guard = lock.write().unwrap();

        for _ in 0..2 {
            let page = Arc::downcast::<AllocationPage<T>>(guard.clone()).unwrap();

            match page.allocate(val) {
                Ok(v) => return (page, v),
                Err(v) => val = v,
            }
            *guard = Arc::new(AllocationPage::<T>::new(self.page_elements));
        }

        unreachable!();
    }
}

unsafe fn paged_control_block_op<T: 'static>(
    cb: NonNull<SharedPtrControlBlock>,
    op: ControlBlockOp,
) {
    let ctrl_ptr = cb.as_ptr() as *mut PagedControlBlock;

    match op {
        ControlBlockOp::FreeData => unsafe {
            // The control block is staying around, so there is no need to clone the arc guarding the page.
            let cb = &*ctrl_ptr;
            let page: &dyn Any = &*cb.page;
            page.downcast_ref::<AllocationPage<T>>()
                .unwrap()
                .deallocate(cb.base.original_data.cast());
        },
        ControlBlockOp::FreeControlBlock => {
            // For this one, we want to make sure the page outlives the freeing of the control block. If this is the
            // last one on the page, the page will go away.
            let page = (*ctrl_ptr).control_page.clone();
            page.unwrap().deallocate(cb.cast());
        }
    }
}

impl SharedPtrAllocStrategy for PagedStrategy {
    fn do_alloc<T: Send + Sync + 'static>(
        &self,
        val: T,
    ) -> (NonNull<SharedPtrControlBlock>, NonNull<T>) {
        let (page, data) = self.alloc_data(val);
        let block = self.alloc_control_block::<T>(data, page);
        (block.cast(), data.cast())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paged_alloc() {
        let allocator = PagedStrategy::new(1024, 1024);

        let ptr = SharedPtr::new(&allocator, 5);
        assert_eq!(*ptr, 5);
    }

    #[test]
    fn test_filling() {
        let alloc = PagedStrategy::new(10, 10);
        // if we free pointers, we never go past the end.
        let mut ptrs = vec![];

        for i in 0..1000usize {
            ptrs.push(SharedPtr::new(&alloc, i));
        }

        for (i, ptr) in ptrs.iter().enumerate() {
            assert_eq!(**ptr, i);
        }
    }
}

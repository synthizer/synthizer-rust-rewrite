use std::ptr::NonNull;
use std::sync::atomic::AtomicUsize;

use crate::shared_ptr::*;

/// An allocation strategy which works by using a simple [Box].
///
/// This is useful for large objects and for testing.
pub(crate) struct SimpleBoxStrategy;

unsafe fn simple_box_control_callback<T>(cb: NonNull<SharedPtrControlBlock>, op: ControlBlockOp) {
    match op {
        ControlBlockOp::FreeData => unsafe {
            std::mem::drop(Box::<T>::from_raw(
                cb.as_ref().original_data.as_ptr() as *mut T
            ));
        },
        ControlBlockOp::FreeControlBlock => unsafe {
            std::mem::drop(Box::<SharedPtrControlBlock>::from_raw(cb.as_ptr()));
        },
    }
}

impl SharedPtrAllocStrategy for SimpleBoxStrategy {
    fn do_alloc<T: Send + Sync + 'static>(
        &self,
        val: T,
    ) -> (NonNull<SharedPtrControlBlock>, NonNull<T>) {
        let data = Box::into_raw(Box::<T>::new(val));
        let control_block = Box::into_raw(Box::new(SharedPtrControlBlock {
            control_callback: simple_box_control_callback::<T>,
            original_data: unsafe { NonNull::new_unchecked(data.cast()) },
            refcount: AtomicUsize::new(1),
            strong_refcount: AtomicUsize::new(1),
        }));

        unsafe {
            (
                NonNull::new_unchecked(control_block),
                NonNull::new_unchecked(data),
            )
        }
    }
}

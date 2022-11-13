use std::ptr::NonNull;
use std::sync::atomic::AtomicUsize;

use crate::shared_ptr::*;

/// An allocation strategy which works by using a simple [Box].
///
/// This is useful for large objects and for testing.
pub(crate) struct SimpleBoxStrategy;

unsafe fn simple_box_free_val<T>(
    _alloc: NonNull<i8>,
    _alloc_arg: Option<NonNull<i8>>,
    val: NonNull<i8>,
) {
    unsafe {
        std::mem::drop(Box::<T>::from_raw(val.as_ptr() as *mut T));
    }
}

unsafe fn simple_box_free_ctl(
    _alloc: NonNull<i8>,
    _alloc_arg: Option<NonNull<i8>>,
    ctl: NonNull<i8>,
) {
    unsafe {
        std::mem::drop(Box::<SharedPtrControlBlock>::from_raw(
            ctl.as_ptr() as *mut SharedPtrControlBlock
        ));
    }
}

impl SharedPtrAllocStrategy for SimpleBoxStrategy {
    fn do_alloc<T: Send + Sync + 'static>(
        &self,
        val: T,
    ) -> (NonNull<SharedPtrControlBlock>, NonNull<T>) {
        let data = Box::into_raw(Box::<T>::new(val));
        let control_block = Box::into_raw(Box::new(SharedPtrControlBlock {
            type_id: std::any::TypeId::of::<T>(),
            allocator: NonNull::dangling(),
            allocator_arg: None,
            free_callback: simple_box_free_val::<T>,
            original_data: unsafe { NonNull::new_unchecked(data as *mut i8) },
            free_control_callback: simple_box_free_ctl,
            // We have to fix this one up at the end once we know the address of the control block itself.
            free_control_arg: NonNull::dangling(),
            refcount: AtomicUsize::new(1),
            strong_refcount: AtomicUsize::new(1),
        }));

        unsafe {
            control_block.as_mut().unwrap().free_control_arg =
                NonNull::new_unchecked(control_block as *mut i8);
        }

        unsafe {
            (
                NonNull::new_unchecked(control_block),
                NonNull::new_unchecked(data),
            )
        }
    }
}

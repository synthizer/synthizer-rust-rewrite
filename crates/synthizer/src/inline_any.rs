use std::any::{Any, TypeId};
use std::mem::MaybeUninit;
use std::mem::{align_of, size_of};

use crate::deferred_freeing::DeferredFree;

const SMALL_SIZE: usize = 128;
const SMALL_ALIGN: usize = 16;

/// This is like `Box<dyn Any>` but any type which is <128 bytes and without an alignment over 16 is stored inline.
pub(crate) struct InlineAny {
    type_id: TypeId,
    content: Content,
}

#[repr(C, align(16))]
struct AlignedSmall {
    data: [u8; SMALL_SIZE],

    /// When dropped, called with a pointer to the data field.  See [copy_free].
    free: unsafe fn(*const u8),
}
struct Big {
    data: Option<std::ptr::NonNull<i8>>,
    free: unsafe fn(*mut i8),
}

enum Content {
    Small(AlignedSmall),
    Big(Big),
}

/// Drop something by copying it onto the stack and then letting it go.
unsafe fn copy_drop<T>(data: *const u8) {
    unsafe {
        let mut stack = MaybeUninit::<T>::uninit();
        let raw_ptr = stack.as_mut_ptr() as *mut u8;
        std::mem::drop(stack.assume_init());
        std::ptr::copy(data, raw_ptr, size_of::<T>());
    }
}

unsafe fn drop_box<T>(data: *mut i8) {
    let b: Box<T> = Box::from_raw(data as *mut T);
    std::mem::drop(b);
}

impl InlineAny {
    pub(crate) fn new<T: Any + Send + Sync + 'static>(val: T) -> Self {
        let content = if size_of::<T>() <= SMALL_SIZE && align_of::<T>() <= SMALL_ALIGN {
            let mut bytes = AlignedSmall {
                data: [0; SMALL_SIZE],

                free: copy_drop::<T>,
            };

            let src_ptr = &val as *const T as *const u8;
            unsafe {
                std::ptr::copy(src_ptr, &mut bytes.data as *mut u8, size_of::<T>());
            }

            // We now own the data, so forget about the one on the stack.
            std::mem::forget(val);
            Content::Small(bytes)
        } else {
            Content::Big(Big {
                data: std::ptr::NonNull::new(Box::into_raw(Box::new(val)) as *mut i8),
                free: drop_box::<T>,
            })
        };

        Self {
            type_id: TypeId::of::<T>(),
            content,
        }
    }

    fn get_ref<T: Any + Send + Sync + 'static>(&self) -> Option<&T> {
        if self.type_id != TypeId::of::<T>() {
            return None;
        }

        let out = unsafe {
            match self.content {
                Content::Small(ref s) => &*(s.data.as_ptr() as *const T),
                Content::Big(ref b) => &*(b.data.unwrap().as_ptr() as *const T),
            }
        };

        Some(out)
    }

    fn get_mut<T: Any + Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        if self.type_id != TypeId::of::<T>() {
            return None;
        }

        let out = unsafe {
            match self.content {
                Content::Small(ref mut s) => &mut *(s.data.as_mut_ptr() as *mut T),
                Content::Big(ref mut b) => &mut *(b.data.unwrap().as_ptr() as *mut T),
            }
        };

        Some(out)
    }
}

impl Drop for AlignedSmall {
    fn drop(&mut self) {
        unsafe { (self.free)(&self.data as *const u8) };
    }
}

impl Drop for Big {
    fn drop(&mut self) {
        if let Some(p) = self.data.take() {
            unsafe { (self.free)(p.as_ptr()) }
        }
    }
}

unsafe impl DeferredFree for InlineAny {
    fn defer_free(mut self, executor: &dyn crate::deferred_freeing::DeferredFreeExecutor) {
        if let Content::Big(ref mut b) = self.content {
            // the take makes sure we don't double free when dropping.
            unsafe { executor.free_cb(b.free, b.data.take().unwrap().as_ptr()) };
        }
    }
}

// These are safe because of the type parameters on the methods; if we ever add a method that allows a `T` that is not Send or Sync, these become invalid.
unsafe impl Send for InlineAny {}
unsafe impl Sync for InlineAny {}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::deferred_freeing::ImmediateExecutor;

    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn test_small() {
        static DROPPED: AtomicU64 = AtomicU64::new(0);

        struct SmallPayload(u64);

        impl Drop for SmallPayload {
            fn drop(&mut self) {
                DROPPED.fetch_add(1, Ordering::Relaxed);
            }
        }

        let mut d = InlineAny::new(SmallPayload(1));

        assert_eq!(d.get_ref::<SmallPayload>().unwrap().0, 1);
        d.get_mut::<SmallPayload>().unwrap().0 += 1;
        assert_eq!(d.get_ref::<SmallPayload>().unwrap().0, 2);

        assert!(d.get_ref::<String>().is_none());
        assert!(d.get_mut::<String>().is_none());

        std::mem::drop(d);

        assert_eq!(DROPPED.load(Ordering::Relaxed), 1);

        let d2 = InlineAny::new(SmallPayload(1));
        d2.defer_free(&ImmediateExecutor);
        assert_eq!(DROPPED.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_big() {
        static DROPPED: AtomicU64 = AtomicU64::new(0);

        struct BigPayload(u64, [u8; 4096]);

        impl Drop for BigPayload {
            fn drop(&mut self) {
                DROPPED.fetch_add(1, Ordering::Relaxed);
            }
        }

        let mut d = InlineAny::new(BigPayload(1, [0; 4096]));

        assert_eq!(d.get_ref::<BigPayload>().unwrap().0, 1);
        d.get_mut::<BigPayload>().unwrap().0 += 1;
        assert_eq!(d.get_ref::<BigPayload>().unwrap().0, 2);

        assert!(d.get_ref::<String>().is_none());
        assert!(d.get_mut::<String>().is_none());

        std::mem::drop(d);

        assert_eq!(DROPPED.load(Ordering::Relaxed), 1);

        let d2 = InlineAny::new(BigPayload(1, [0; 4096]));
        d2.defer_free(&ImmediateExecutor);
        assert_eq!(DROPPED.load(Ordering::Relaxed), 2);
    }
}

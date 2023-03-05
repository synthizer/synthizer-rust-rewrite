/// A callback which knows how to free some data.
pub(crate) type DeferredFreeCallback = unsafe fn(*mut i8);

/// Something which knows how to free based on a callback.
pub(crate) trait DeferredFreeExecutor {
    unsafe fn free_cb(&self, callback: DeferredFreeCallback, data: *mut i8);
}

/// Something which knows how to defer cleanup.
///
/// # Safety
///
/// If the callback and the argument passed to it do something bad when passed to the executor, then bad things happen.
pub(crate) unsafe trait DeferredFree {
    fn defer_free(self, executor: &dyn DeferredFreeExecutor);
}

/// A wrapper over something which will defer its cleanup.
///
/// Derefs to the contents.  If this type is dropped without having been deferred, a panic results.
pub(crate) struct DeferredFreeCell<T: DeferredFree>(Option<T>);

impl<T: DeferredFree> DeferredFreeCell<T> {
    pub(crate) fn new(what: T) -> Self {
        Self(Some(what))
    }
}

unsafe impl<T: DeferredFree> DeferredFree for DeferredFreeCell<T> {
    fn defer_free(mut self, executor: &dyn DeferredFreeExecutor) {
        let inner = self.0.take().unwrap();
        inner.defer_free(executor);
    }
}

impl<T: DeferredFree> Drop for DeferredFreeCell<T> {
    fn drop(&mut self) {
        assert!(
            self.0.is_none(),
            "DeferredFreeCell should always be freed via the DeferredFree interface"
        );
    }
}

/// This executor calls the callbacks inline.
pub(crate) struct ImmediateExecutor;

impl DeferredFreeExecutor for ImmediateExecutor {
    unsafe fn free_cb(&self, callback: DeferredFreeCallback, data: *mut i8) {
        callback(data);
    }
}

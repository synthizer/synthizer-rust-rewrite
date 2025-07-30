use std::sync::Arc;

/// Internal helper type: flips a boolean to true when dropped.
///
/// This allows dropping outside the batch context. The object(s) are marked dropped and then, on the next batch, they
/// are actually removed.  Eventually they drop for real, as states rotate out of the audio thread.
pub(crate) struct MarkDropped(pub(crate) Arc<std::sync::atomic::AtomicBool>);

impl MarkDropped {
    pub(crate) fn new() -> Self {
        MarkDropped(Arc::new(std::sync::atomic::AtomicBool::new(false)))
    }
}

impl Drop for MarkDropped {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

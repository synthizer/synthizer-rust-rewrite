use std::sync::RwLock;

static MINIAUDIO_LOCK: RwLock<()> = RwLock::new(());

/// Ensure that miniaudio is initialized, then call the specified closure.
pub(crate) fn dispatch<R>(closure: impl FnOnce() -> R) -> crate::errors::Result<R> {
    crate::initialization::ensure_initialized()?;
    let _guard = MINIAUDIO_LOCK.read().unwrap();
    Ok(closure())
}

/// Ensure that Miniaudio is initialized, then call the specified closure such that it is guaranteed that no other call
/// to Miniaudio is going on concurrently.
///
/// This is most notably needed for device opening, or any other place where Miniaudio tells us something isn't
/// threadsafe.
pub(crate) fn dispatch_exclusive<R>(closure: impl FnOnce() -> R) -> crate::errors::Result<R> {
    crate::initialization::ensure_initialized()?;
    let _guard = MINIAUDIO_LOCK.write().unwrap();
    Ok(closure())
}

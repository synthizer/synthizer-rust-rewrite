thread_local! {
    static IS_AUDIO_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) fn is_audio_thread() -> bool {
    IS_AUDIO_THREAD.with(|x| x.get())
}

/// Mark this thread as being an audio thread, which tells the library to defer or otherwise not perform operations that
/// may block or enter the kernel.
#[inline(always)]
pub(crate) fn mark_audio_thread() {
    IS_AUDIO_THREAD.with(|x| x.replace(true));
}

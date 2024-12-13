//! This module implements background dropping via a background thread and an enum which contains the items to drop.
//!
//! This abstraction lets one  drop things from the audio thread when those things may perform non-realtime-safe
//! operations.  It lags slightly, and will drop on the audio thread as a last resort should a (large) internal buffer
//! fill up.
//!
//! Audio threads must mark themselves with [crate::audio_thread::mark_audio_thread]. If they don't, they will drop on
//! the audio thread.  This is done so that the queues won't experience pressure from non-audio threads which wish to
//! drop things that are sometimes on the audio thread.  As an example, it is important never to drop an Arc from the
//! audio thread, but the Arc may have another reference on a non-audio thread.
use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use thingbuf::StaticThingBuf;

use crate::is_audio_thread::is_audio_thread;

/// The number of items allowed to be pending a drop.
const BACKLOG: usize = 10 * 1024;

/// The number of times to std::thread::yield in the backoff loop of the worker thread when crossbeam says to stop spinning before sleeping.
const YIELDS: usize = 5;

/// The amount of time to sleep when no work has been found for a long time.
///
/// Note that there is no real benefit to going under 15ms because 15ms is often the minimum timer resolution of
/// Windows.
const SLEEP: Duration = Duration::from_millis(20);

static WORK_QUEUE: StaticThingBuf<
    Option<DropElement>,
    BACKLOG,
    crate::option_recycler::OptionRecycler,
> = StaticThingBuf::<Option<DropElement>, BACKLOG, crate::option_recycler::OptionRecycler>::with_recycle(crate::option_recycler::OptionRecycler);

#[derive(derivative::Derivative)]
#[derivative(Debug)]
struct ArcDrop(#[derivative(Debug = "ignore")] Arc<dyn Any + Send + Sync + 'static>);

#[derive(derivative::Derivative)]
#[derivative(Debug)]
struct BoxDrop(#[derivative(Debug = "ignore")] Box<dyn Any + Send + Sync + 'static>);

macro_rules! decl_variant {
    ($($tys:ident),*)=> {
        mod drop_variant {



            use super::*;

            variant!(pub(super) DropElement, $($tys),*);


            $(
                impl BackgroundDroppable for $tys {
                    fn background_drop(self) {
                        enqueue_or_drop(self.into());
                    }
                }
            )*
        }

        use drop_variant::*;

    }
}

decl_variant!(ArcDrop, BoxDrop);

/// When background_drop is called, drop the item on the background thread if needed.
pub(crate) trait BackgroundDroppable {
    fn background_drop(self);
}

/// Types wrapped in this wrapper will drop on a background thread.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd, Debug, Hash)]
pub(crate) struct BackgroundDrop<T: BackgroundDroppable> {
    inner: Option<T>,
}

impl<T: BackgroundDroppable> BackgroundDrop<T> {
    pub fn new(val: T) -> Self {
        Self { inner: Some(val) }
    }
}

impl<T: BackgroundDroppable> std::ops::Deref for BackgroundDrop<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.inner.as_ref().unwrap_unchecked() }
    }
}

impl<T: BackgroundDroppable> std::ops::DerefMut for BackgroundDrop<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.inner.as_mut().unwrap_unchecked() }
    }
}

impl<T: BackgroundDroppable> Drop for BackgroundDrop<T> {
    fn drop(&mut self) {
        // This branch will be optimized out as needed and should thus be first.
        if !std::mem::needs_drop::<T>() {
            // Nothing to do, so stop early and don't put backpressure on the queue.
            return;
        }

        if !is_audio_thread() {
            // It's not an audio thread. Nothing to do.
            return;
        }

        let inner = unsafe { self.inner.take().unwrap_unchecked() };
        inner.background_drop();
    }
}

/// called when a server facing the audio device is created, to make sure that the background thread is running.
pub fn ensure_background_drop_thread_started() {
    use std::sync::Once;

    static THREAD_STARTED: Once = Once::new();
    THREAD_STARTED.call_once(|| {
        std::thread::spawn(background_drop_worker);
    });
}

fn background_drop_worker() {
    let mut no_work_counter = 0;
    loop {
        if let Some(w) = WORK_QUEUE.pop() {
            std::mem::drop(w);
            no_work_counter = 0;
            continue;
        }

        no_work_counter += 1;

        if no_work_counter < YIELDS {
            std::thread::yield_now();
        } else {
            std::thread::sleep(SLEEP);
        }
    }
}

fn enqueue_or_drop(element: DropElement) {
    let x = WORK_QUEUE.push(Some(element));
    std::mem::drop(x);
}

// Now we must punch out a bunch of impls of BackgroundDroppable.

impl<T: Any + Send + Sync + 'static> BackgroundDroppable for Arc<T> {
    fn background_drop(self) {
        let will_drop = ArcDrop(self as Arc<_>);
        enqueue_or_drop(will_drop.into());
    }
}

impl BackgroundDroppable for Arc<dyn Any + Send + Sync + 'static> {
    fn background_drop(self) {
        let will_drop = ArcDrop(self);
        enqueue_or_drop(will_drop.into());
    }
}

impl<T: Any + Send + Sync + 'static> BackgroundDroppable for Box<T> {
    fn background_drop(self) {
        let will_drop = BoxDrop(self as Box<_>);
        enqueue_or_drop(will_drop.into());
    }
}

impl BackgroundDroppable for Box<dyn Any + Send + Sync + 'static> {
    fn background_drop(self) {
        let will_drop = BoxDrop(self);
        enqueue_or_drop(will_drop.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Threads which are not audio threads should drop inline.
    #[test]
    fn test_no_drop_on_background() {
        ensure_background_drop_thread_started();

        thread_local! {
            static THIS_THREAD: std::cell::Cell<bool>=const{std::cell::Cell::new(false)};
        };

        struct InlineDrop(u32);

        impl Drop for InlineDrop {
            fn drop(&mut self) {
                THIS_THREAD.with(|x| x.replace(true));
            }
        }

        let boxed = Box::new(InlineDrop(5));
        let wrapped = BackgroundDrop::new(boxed);

        assert!(!THIS_THREAD.with(|x| x.get()));
        std::mem::drop(wrapped);
        assert!(THIS_THREAD.with(|x| x.get()));
    }

    /// This should defer the drop.  Note that, importantly, the drop must happen.
    #[test]
    fn test_deferred_drop() {
        ensure_background_drop_thread_started();

        struct Dropping(u32);

        static mut DID_DROP: bool = false;
        thread_local! {
            static THIS_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        }

        impl Drop for Dropping {
            fn drop(&mut self) {
                unsafe { DID_DROP = true };
                THIS_THREAD.with(|x| x.replace(true));
            }
        }

        // This must all happen in another thread.  If not, then the test threads become audio threads.
        std::thread::spawn(|| {
            crate::is_audio_thread::mark_audio_thread();
            let boxed = Box::new(Dropping(5));
            let wrapped = BackgroundDrop::new(boxed);
            std::mem::drop(wrapped);
            assert!(!THIS_THREAD.with(|x| x.get()));
        })
        .join()
        .unwrap();

        // Ok, but we should still drop eventually.
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(unsafe { DID_DROP });
    }
}

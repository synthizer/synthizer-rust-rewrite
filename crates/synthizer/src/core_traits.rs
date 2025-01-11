#![allow(private_interfaces)]
use std::any::Any;
use std::sync::Arc;

use crate::context::*;
use crate::error::Result;
use crate::sample_sources::execution::Executor as MediaExecutor;
use crate::unique_id::UniqueId;

// These are "core" but it's a lot of code so we pull it out.
pub(crate) use crate::value_provider::*;

pub(crate) mod sealed {
    use super::*;

    /// Traced resources.
    pub enum TracedResource {
        Slot(Arc<dyn Any + Send + Sync + 'static>),
        Media(Arc<MediaExecutor>),
    }

    /// This internal trait is the actual magic.
    ///
    /// # Safety
    ///
    /// This trait is unsafe because the library relies on it to uphold the contracts documented with the method.  This
    /// lets us get performance out, especially in debug builds where things like immediate unwrapping of options will
    /// not be optimized away.
    ///
    /// See also the documentation on [ValueProvider].
    pub unsafe trait Signal: Sized + Send + Sync + 'static {
        type Input<'il>: Sized;
        type Output<'ol>: Sized;
        type State: Sized + Send + Sync + 'static;

        /// Tick this signal.
        ///
        /// Exactly `BLOCK_SIZE` ticks will occur between calls to `on_block_start`.  They may be broken up into smaller
        /// blocks, possibly down to 1 sample (for example, in recursive structures).
        ///
        /// Signals have two opportunities to perform work.  The first is in the body of this method.  The second is in
        /// the value provider returned from this method.  So:
        ///
        /// - If the signal has a side effect or needs to compute the entire sequence to know what the next value is
        ///   (e.g. convolution), it must do work in this method.  It is not guaranteed if or in what order the provider
        ///   will be used.
        /// - If the signal can compute any arbitrary value in this block, it may elect to do work in the output
        ///   provider.  For example, `sin(x)` is effectively a map over the parent signal's output provider and, save
        ///   for the possibility of duplicate work, it may simply perform the map-like operation.
        ///
        /// For these reasons, it's important that signals not (ab)use the ability to use the same index in a provider
        /// multiple times.  We allow it, but it may duplicate work.
        ///
        /// For a concrete example of why this matters, consider signals where we only care about the output for the
        /// first sample of every block.  This is common to do when doing automation, since it's expensive to, e.g.,
        /// redesign filters on every sample.  In this case, `tick` is called some number of times, but the provider
        /// would only be used on the first tick call at the beginning of the block, and otherwise simply dropped.
        fn tick<'il, 'ol, 's, I, const N: usize>(
            ctx: &'_ SignalExecutionContext<'_, '_>,
            input: I,
            state: &'s mut Self::State,
        ) -> impl ValueProvider<Self::Output<'ol>>
        where
            Self::Input<'il>: 'ol,
            'il: 'ol,
            's: 'ol,
            I: ValueProvider<Self::Input<'il>> + Sized;

        /// Called when a signal is starting a new block.
        ///
        /// This will be called every [config::BLOCK_SIZE] ticks.  All signals wrapping other signals must call it on
        /// their wrapped signals.  Only "leaf" signals may ignore it.  It is entirely correct to do nothing here.  This
        /// is used for many things, among them gathering references to buses or resetting block-based counters.
        ///
        /// No default impl is provided.  All signals need to consider what they want to do so we force the issue.
        fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State);
    }

    /// A frame of audio data, which can be stored on the stack.
    pub trait AudioFrame<T>
    where
        T: Copy + Default,
        Self: Clone,
    {
        /// Make a default/empty/whatever frame.
        ///
        /// We can't use Default because it's not implemented for all arrays and tuples due to backward compatibility in
        /// stdlib.
        fn default_frame() -> Self;

        fn channel_count(&self) -> usize;
        fn get(&self, index: usize) -> &T;
        fn get_mut(&mut self, index: usize) -> &mut T;
        fn set(&mut self, index: usize, value: T);

        fn get_or_default(&self, index: usize) -> T {
            if index > self.channel_count() {
                Default::default()
            } else {
                *self.get(index)
            }
        }

        fn set_or_ignore(&mut self, index: usize, value: T) {
            if index < self.channel_count() {
                self.set(index, value);
            }
        }
    }

    pub struct ReadySignal<SigT, StateT> {
        pub(crate) signal: SigT,
        pub(crate) state: StateT,
    }

    /// Something which knows how to convert itself into a signal.
    ///
    /// You actually build signals up with these, not with the signal traits directly.
    ///
    /// Again, this trait is in practice sealed.
    pub trait IntoSignal {
        type Signal: Signal;

        fn into_signal(self) -> Result<ReadySignal<Self::Signal, IntoSignalState<Self>>>;

        /// Trace a signal's resource usage, and allocate objects.
        ///
        /// The synthesizer must use erased `Any` to store objects in the maps.  That means that it is necessary to let
        /// signals allocate such objects, then hand them off.  It's also required that we trace signals to figure out
        /// other things, for example which things might be used before others.
        ///
        /// Implementations should:
        ///
        /// - If a combinator or other signal with "parents": call this on the parents, in the order that the signal
        ///   would call `tick` on those parents.
        /// - If a "leaf" which uses resources (e.g. slots) call the callback.
        /// - If a "leaf" which doesn't need resources, add an empty impl.
        /// - If a combinator which uses resources, call the tracer either before calling the parents (if the resource
        ///   is used before ticking them) or after (if the resource is used after).  Using resources "in the middle"
        ///   should be avoided.
        fn trace<F: FnMut(UniqueId, TracedResource)>(&mut self, inserter: &mut F) -> Result<()>;
    }

    pub(crate) type IntoSignalResult<S> =
        Result<ReadySignal<<S as IntoSignal>::Signal, IntoSignalState<S>>>;
}

pub(crate) use sealed::*;

// Workarounds for https://github.com/rust-lang/rust/issues/38078: rustc is not always able to determine when a type
// isn't ambiguous, or at the very least it doesn't tell us what the options are, so we use this instead.
pub(crate) type IntoSignalOutput<'a, S> = <<S as IntoSignal>::Signal as Signal>::Output<'a>;
pub(crate) type IntoSignalInput<'a, S> = <<S as IntoSignal>::Signal as Signal>::Input<'a>;
pub(crate) type IntoSignalState<S> = <<S as IntoSignal>::Signal as Signal>::State;
pub(crate) type SignalInput<'a, T> = <T as Signal>::Input<'a>;
pub(crate) type SignalOutput<'a, T> = <T as Signal>::Output<'a>;
pub(crate) type SignalState<S> = <S as Signal>::State;

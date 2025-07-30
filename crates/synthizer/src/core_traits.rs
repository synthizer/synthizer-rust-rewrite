#![allow(private_interfaces)]
use std::any::Any;
use std::sync::Arc;

use crate::context::*;
use crate::error::Result;
use crate::synthesizer::AudioThreadState;
use crate::unique_id::UniqueId;

// These are "core" but it's a lot of code so we pull it out.
// Value providers are still used internally but not in the Signal trait anymore

pub(crate) mod sealed {
    use super::*;

    /// Traced resources.
    pub enum TracedResource {
        Slot(Arc<dyn Any + Send + Sync + 'static>),
    }

    /// This internal trait is the actual magic.
    ///
    /// # Safety
    ///
    /// This trait is unsafe because the library relies on it to uphold the contracts documented with the method.  This
    /// lets us get performance out, especially in debug builds where things like immediate unwrapping of options will
    /// not be optimized away.
    pub unsafe trait Signal: Sized + Send + Sync + 'static {
        type Input: Sized;
        type Output: Sized;
        type State: Sized + Send + Sync + 'static;

        /// Process a single frame of audio.
        ///
        /// This will be called exactly `BLOCK_SIZE` times between calls to `on_block_start`. The implementation should
        /// be as efficient as possible since this is the hot path.
        ///
        /// For signals that need to process multiple frames at once (e.g., convolution), they should buffer internally
        /// in their state.
        fn tick_frame(
            ctx: &'_ SignalExecutionContext<'_, '_>,
            input: Self::Input,
            state: &mut Self::State,
        ) -> Self::Output;

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
        /// signals allocate such objects, then hand them off.
        ///
        /// Implementations should:
        ///
        /// - If a combinator or other signal with "parents": call this on the parents, in the order that the signal
        ///   would call `tick` on those parents.
        /// - If a "leaf" which uses resources (e.g. slots) call the callback.
        /// - If a "leaf" which doesn't need resources, add an empty impl.
        fn trace<F: FnMut(UniqueId, TracedResource)>(&mut self, inserter: &mut F) -> Result<()>;
    }

    pub(crate) type IntoSignalResult<S> =
        Result<ReadySignal<<S as IntoSignal>::Signal, IntoSignalState<S>>>;
}

pub(crate) use sealed::*;

// Workarounds for https://github.com/rust-lang/rust/issues/38078: rustc is not always able to determine when a type
// isn't ambiguous, or at the very least it doesn't tell us what the options are, so we use this instead.
pub(crate) type IntoSignalOutput<S> = <<S as IntoSignal>::Signal as Signal>::Output;
pub(crate) type IntoSignalInput<S> = <<S as IntoSignal>::Signal as Signal>::Input;
pub(crate) type IntoSignalState<S> = <<S as IntoSignal>::Signal as Signal>::State;
pub(crate) type SignalInput<T> = <T as Signal>::Input;
pub(crate) type SignalOutput<T> = <T as Signal>::Output;
pub(crate) type SignalState<S> = <S as Signal>::State;

/// Trait for commands that can be executed on the audio thread.
pub trait Command: Send + 'static {
    fn execute(&mut self, state: &mut AudioThreadState);
}

/// Blanket implementation for FnMut closures
impl<F> Command for F
where
    F: FnMut(&mut AudioThreadState) + Send + 'static,
{
    fn execute(&mut self, state: &mut AudioThreadState) {
        self(state)
    }
}

use std::any::Any;
use std::sync::Arc;

use crate::context::*;
use crate::error::Result;
use crate::unique_id::UniqueId;

// These are "core" but it's a lot of code so we pull it out.
pub(crate) use crate::value_provider::*;

pub(crate) mod sealed {
    use super::*;

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
        fn tick<'il, 'ol, I, const N: usize>(
            ctx: &'_ SignalExecutionContext<'_, '_>,
            input: I,
            state: &mut Self::State,
        ) -> impl ValueProvider<Self::Output<'ol>>
        where
            Self::Input<'il>: 'ol,
            'il: 'ol,
            I: ValueProvider<Self::Input<'il>> + Sized;

        /// Called when a signal is starting a new block.
        ///
        /// This will be called every [config::BLOCK_SIZE] ticks.  All signals wrapping other signals must call it on
        /// their wrapped signals.  Only "leaf" signals may ignore it.  It is entirely correct to do nothing here.  This
        /// is used for many things, among them gathering references to buses or resetting block-based counters.
        ///
        /// No default impl is provided.  All signals need to consider what they want to do so we force the issue.
        fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State);

        /// Trace slots.
        ///
        /// This is "private" to the slot machinery, but must be implemented on combinators.  Combinators should forward
        /// to their parents.  Everything else should leave the implementation empty.
        ///
        /// This is called when mounting, in the thread that mounts.  It calls the callback with ids and states for new
        /// slots.  The only implementor which does anything but pass to other signals is `SlotSignal`.
        ///
        /// If the user tries to use a slot which is not traced they get an error.  If the algorithm tries to use a slot
        /// which is not traced, we panic.  The latter is an internal bug.  It is on us to always know what slots the
        /// user made.
        ///
        /// The callback gets called with an Arc to the *value* of the slot.  The rest is wrapped up by the generic
        /// machinery.
        fn trace_slots<F: FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>)>(
            state: &Self::State,
            inserter: &mut F,
        );
    }

    pub trait SignalDestination<Input: Sized, const N: usize> {
        fn send(self, values: [Input; N]);
    }

    /// A frame of audio data, which can be stored on the stack.
    pub trait AudioFrame<T>
    where
        T: Copy,
    {
        fn channel_count(&self) -> usize;
        fn get(&self, index: usize) -> &T;
        fn set(&mut self, index: usize, value: T);
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
    }

    pub(crate) type IntoSignalResult<S> =
        Result<ReadySignal<<S as IntoSignal>::Signal, IntoSignalState<S>>>;
}

pub(crate) use sealed::*;

impl<F, Input, const N: usize> SignalDestination<Input, N> for F
where
    Input: Sized,
    F: FnOnce([Input; N]),
{
    fn send(self, value: [Input; N]) {
        self(value)
    }
}

pub trait Generator: for<'a> Signal<Input<'a> = ()> {}
impl<T> Generator for T where T: for<'a> Signal<Input<'a> = ()> {}

/// A mountable signal has no inputs and no outputs, and its state and parameters are 'static.
pub trait Mountable
where
    Self: Generator + Send + Sync + 'static,
    Self: for<'a> Signal<Output<'a> = ()> + Generator,
    SignalState<Self>: Send + Sync + 'static,
{
}

impl<T> Mountable for T
where
    T: Generator + for<'a> Signal<Output<'a> = ()> + Send + Sync + 'static,
    SignalState<T>: Send + Sync + 'static,
{
}

// Workarounds for https://github.com/rust-lang/rust/issues/38078: rustc is not always able to determine when a type
// isn't ambiguous, or at the very least it doesn't tell us what the options are, so we use this instead.
pub(crate) type IntoSignalOutput<'a, S> = <<S as IntoSignal>::Signal as Signal>::Output<'a>;
pub(crate) type IntoSignalInput<'a, S> = <<S as IntoSignal>::Signal as Signal>::Input<'a>;
pub(crate) type IntoSignalState<S> = <<S as IntoSignal>::Signal as Signal>::State;
pub(crate) type SignalInput<'a, T> = <T as Signal>::Input<'a>;
pub(crate) type SignalOutput<'a, T> = <T as Signal>::Output<'a>;
pub(crate) type SignalState<S> = <S as Signal>::State;

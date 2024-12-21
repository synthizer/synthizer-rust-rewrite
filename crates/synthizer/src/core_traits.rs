use std::any::Any;
use std::sync::Arc;

use crate::context::*;
use crate::error::Result;
use crate::unique_id::UniqueId;

pub(crate) mod sealed {
    use super::*;

    /// This internal trait is the actual magic.
    ///
    /// # Safety
    ///
    /// This trait is unsafe because the library relies on it to uphold the contracts documented with the method.  This
    /// lets us get performance out, especially in debug builds where things like immediate unwrapping of options will
    /// not be optimized away.
    pub unsafe trait Signal: Sized + Send + Sync {
        type Input<'il>: Sized;
        type Output<'ol>: Sized;
        type State: Sized + Send + Sync;
        type Parameters: Sized + Send + Sync;

        /// Tick this signal.
        ///
        /// Exactly `BLOCK_SIZE` ticks will occur between calls to `on_block_start`.  They may be broken up into smaller
        /// blocks, possibly down to 1 sample (for example, in recursive structures).
        ///
        /// This method must uphold two important invariants:
        ///
        /// - `input(i)` must be called at minimum once on each input index in the range `0..N`.
        /// - Exactly `N` outputs are sent to the destination.
        ///
        /// Signals may choose to do work in either of those points instead, so they must be used to drive dependent
        /// signals.
        fn tick<'il, 'ol, D, const N: usize>(
            ctx: &'_ SignalExecutionContext<'_, '_>,
            input: [Self::Input<'il>; N],
            params: &Self::Parameters,
            state: &mut Self::State,
            destination: D,
        ) where
            D: SignalDestination<Self::Output<'ol>, N>,
            Self::Input<'il>: 'ol,
            'il: 'ol;

        /// Called when a signal is starting a new block.
        ///
        /// This will be called every [config::BLOCK_SIZE] ticks.  All signals wrapping other signals must call it on
        /// their wrapped signals.  Only "leaf" signals may ignore it.  It is entirely correct to do nothing here.  THis
        /// is used for many things, among them gathering references to buses or resetting block-based counters.
        ///
        /// No default impl is provided.  All signals need to consider what they want to do so we forc3e the issue.
        fn on_block_start(
            ctx: &SignalExecutionContext<'_, '_>,
            params: &Self::Parameters,
            state: &mut Self::State,
        );

        /// Trace slots.
        ///
        /// This is "private" to the slot machinery, but must e implemented on combinators.  Everything else should
        /// leave the implementation empty.
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
            parameters: &Self::Parameters,
            inserter: &mut F,
        );
    }

    pub trait SignalDestination<Input: Sized, const N: usize> {
        fn send(self, values: [Input; N]);
    }

    /// A frame of audio data, which can be stored on the stack.
    ///
    /// Frames are basically scalars used to pass audio data around on the stack without taking the hit of an
    /// allocation.  They are immutable after creation and always `f64`.  f64 scalars are single-channel frames.
    ///
    /// # Safety
    ///
    /// This trait is unsafe to implement because frames must always fill their destination by calling the closure
    ///   exactly the number of times their channel counts say they have channels.
    ///
    /// In other words, it's not wrong to think of this a bit like a SIMD vector or something; we'd use const if we
    /// could but that's not stable enough for our purposes, and some frames may have a runtime specified format and
    /// size in any case.
    ///
    /// Furthermore, if a signal outputs frames it must not change the  channel count until the next block. That's
    /// really not specifically for this trait though; see [Signal] for more on how formats work.  We don't tie formats
    /// to frames but rather to signals.
    pub unsafe trait AudioFrame {
        fn channel_count(&self) -> usize;

        fn read_one<F: FnOnce(f64)>(&self, channel: usize, destination: F);

        /// This is the unsafe-to-implement method: it must call the closure exactly `self.channel_count()` times, no
        /// more or less.
        ///
        /// The default implementation delegates to read_one.
        fn read_all<F: FnMut(f64)>(&self, mut destination: F) {
            for i in 0..self.channel_count() {
                self.read_one(i, &mut destination);
            }
        }
    }

    pub struct ReadySignal<SigT, StateT, ParamsT> {
        pub(crate) signal: SigT,
        pub(crate) state: StateT,
        pub(crate) parameters: ParamsT,
    }

    /// Something which knows how to convert itself into a signal.
    ///
    /// You actually build signals up with these, not with the signal traits directly.
    ///
    /// Again, this trait is in practice sealed.
    pub trait IntoSignal {
        type Signal: Signal;

        fn into_signal(
            self,
        ) -> Result<ReadySignal<Self::Signal, IntoSignalState<Self>, IntoSignalParameters<Self>>>;
    }

    pub(crate) type IntoSignalResult<S> =
        Result<ReadySignal<<S as IntoSignal>::Signal, IntoSignalState<S>, IntoSignalParameters<S>>>;
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
    SignalParameters<Self>: Send + Sync + 'static,
{
}

impl<T> Mountable for T
where
    T: Generator + for<'a> Signal<Output<'a> = ()> + Send + Sync + 'static,
    SignalState<T>: Send + Sync + 'static,
    SignalParameters<T>: Send + Sync + 'static,
{
}

// Workarounds for https://github.com/rust-lang/rust/issues/38078: rustc is not always able to determine when a type
// isn't ambiguous, or at the very least it doesn't tell us what the options are, so we use this instead.
pub(crate) type IntoSignalOutput<'a, S> = <<S as IntoSignal>::Signal as Signal>::Output<'a>;
pub(crate) type IntoSignalInput<'a, S> = <<S as IntoSignal>::Signal as Signal>::Input<'a>;
pub(crate) type IntoSignalParameters<S> = <<S as IntoSignal>::Signal as Signal>::Parameters;
pub(crate) type IntoSignalState<S> = <<S as IntoSignal>::Signal as Signal>::State;
pub(crate) type SignalInput<'a, T> = <T as Signal>::Input<'a>;
pub(crate) type SignalOutput<'a, T> = <T as Signal>::Output<'a>;
pub(crate) type SignalState<S> = <S as Signal>::State;
pub(crate) type SignalParameters<S> = <S as Signal>::Parameters;

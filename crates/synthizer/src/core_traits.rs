use std::any::Any;
use std::sync::Arc;

use crate::config;
use crate::context::*;
use crate::error::Result;
use crate::unique_id::UniqueId;

pub(crate) mod sealed {
    use super::*;

    /// This internal trait is the actual magic.
    ///
    /// # Safety
    ///
    /// This trait is unsafe because the library relies on it to uphold the contracts documented with the method.  In
    /// particular, calling `tick1` must always send exactly one value to the destination, as the destination may be
    /// writing into uninitialized memory.  This lets us get performance out, especially in debug builds where things
    /// like immediate unwrapping of options will not be optimized away.
    pub unsafe trait Signal: Sized + Send + Sync {
        type Input: Sized;
        type Output: Sized;
        type State: Sized + Send + Sync;
        type Parameters: Sized + Send + Sync;

        /// Tick this signal once.
        ///
        /// Must use the destination to send exactly one value.
        fn tick1<D: SignalDestination<Self::Output>>(
            ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
            input: &'_ Self::Input,
            destination: D,
        );

        /// Tick this signal [config::BLOCK_SIZE] times.  Implemented via `tick1` by default.
        ///
        /// The default implementation is only suitable for signals which are "endpoints" e.g. signals that produce
        /// values from nothing.  Combinators must implement this in terms of their combined signals.  Signals which are
        /// not combinators may ignore this if they are wrapping a signal, but cannot meaningfully benefit from or store
        /// output data.  The complexity here, though: when reading from files and external sources, we are not able to
        /// do so efficiently sample by sample.  Anywhere which does not forward block-based calls to their parent
        /// de-optimizes this path.
        ///
        /// Callers will choose to use tick1 or to use tick_block, and will not mix them for the duration of a block.
        /// Callers may change their mind between blocks.  Wrapper signals should also preserve this property, though
        /// since this is an entire block of data, it's hard to break this rule.
        fn tick_block<
            'a,
            I: FnMut(usize) -> &'a Self::Input,
            D: ReusableSignalDestination<Self::Output>,
        >(
            ctx: &'_ mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
            mut input: I,
            mut destination: D,
        ) where
            Self::Input: 'a,
        {
            for i in 0..config::BLOCK_SIZE {
                Self::tick1(ctx, input(i), |x| destination.send_reusable(x));
            }
        }

        /// Called when a signal is starting a new block.
        ///
        /// This will be called every [config::BLOCK_SIZE] ticks.  All signals wrapping other signals must call it on
        /// their wrapped signals.  Only "leaf" signals may ignore it.  It is entirely correct to do nothing here.  THis
        /// is used for many things, among them gathering references to buses or resetting block-based counters.
        ///
        /// No default impl is provided.  All signals need to consider what they want to do so we forc3e the issue.
        fn on_block_start(ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>);

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

    pub trait SignalDestination<Input: Sized> {
        fn send(self, value: Input);
    }

    pub trait ReusableSignalDestination<Input: Sized>: SignalDestination<Input> {
        fn send_reusable(&mut self, value: Input);
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

impl<F, Input> SignalDestination<Input> for F
where
    Input: Sized,
    F: FnOnce(Input),
{
    fn send(self, value: Input) {
        self(value)
    }
}

impl<F, Input> ReusableSignalDestination<Input> for F
where
    Input: Sized,
    F: FnMut(Input),
{
    fn send_reusable(&mut self, value: Input) {
        (*self)(value)
    }
}

pub trait Generator: Signal<Input = ()> {}
impl<T> Generator for T where T: Signal<Input = ()> {}

/// A mountable signal has no inputs and no outputs, and its state and parameters are 'static.
pub trait Mountable
where
    Self: Generator + Send + Sync + 'static,
    Self: Signal<Output = ()> + Generator,
    SignalState<Self>: Send + Sync + 'static,
    SignalParameters<Self>: Send + Sync + 'static,
{
}

impl<T> Mountable for T
where
    T: Generator + Signal<Output = ()> + Send + Sync + 'static,
    SignalState<T>: Send + Sync + 'static,
    SignalParameters<T>: Send + Sync + 'static,
{
}

// Workarounds for https://github.com/rust-lang/rust/issues/38078: rustc is not always able to determine when a type
// isn't ambiguous, or at the very least it doesn't tell us what the options are, so we use this instead.
pub(crate) type IntoSignalOutput<S> = <<S as IntoSignal>::Signal as Signal>::Output;
pub(crate) type IntoSignalInput<S> = <<S as IntoSignal>::Signal as Signal>::Input;
pub(crate) type IntoSignalParameters<S> = <<S as IntoSignal>::Signal as Signal>::Parameters;
pub(crate) type IntoSignalState<S> = <<S as IntoSignal>::Signal as Signal>::State;
pub(crate) type SignalInput<T> = <T as Signal>::Input;
pub(crate) type SignalOutput<T> = <T as Signal>::Output;
pub(crate) type SignalState<S> = <S as Signal>::State;
pub(crate) type SignalParameters<S> = <S as Signal>::Parameters;

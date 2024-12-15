use crate::config;
use crate::error::Result;

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
    }

    pub trait SignalDestination<Input: Sized> {
        fn send(self, value: Input);
    }
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

pub struct SignalExecutionContext<'a, 'shared, TState, TParameters> {
    pub(crate) state: &'a mut TState,
    pub(crate) parameters: &'a TParameters,

    pub(crate) fixed: &'a mut FixedSignalExecutionContext<'shared>,
}

/// Parts of the execution context which do not contain references that need to be recast.
pub(crate) struct FixedSignalExecutionContext<'a> {
    pub(crate) time: u64,
    pub(crate) audio_destinationh: &'a mut [f64; config::BLOCK_SIZE],
}

impl<'shared, TState, TParameters> SignalExecutionContext<'_, 'shared, TState, TParameters> {
    /// Convert this context into values usually derived from reborrows of this context's fields.  Used to grab parts of
    /// contexts when moving upstream.
    pub(crate) fn wrap<'a, NewS, NewP>(
        &'a mut self,
        new_state: impl FnOnce(&'a mut TState) -> &'a mut NewS,
        new_params: impl FnOnce(&'a TParameters) -> &'a NewP,
    ) -> SignalExecutionContext<'a, 'shared, NewS, NewP>
    where
        'shared: 'a,
    {
        SignalExecutionContext {
            state: new_state(self.state),
            parameters: new_params(self.parameters),
            fixed: self.fixed,
        }
    }
}

pub trait Generator: Signal<Input = ()> {}
impl<T> Generator for T where T: Signal<Input = ()> {}

/// A mountable signal has no inputs and no outputs, and its state and parameters are 'static.
pub trait Mountable
where
    Self: Generator + Send + Sync + 'static,
    Self: Signal<Output = ()> + Generator,
    SignalSealedState<Self>: Send + Sync + 'static,
    SignalSealedParameters<Self>: Send + Sync + 'static,
{
}

impl<T> Mountable for T
where
    T: Generator + Signal<Output = ()> + Send + Sync + 'static,
    SignalSealedState<T>: Send + Sync + 'static,
    SignalSealedParameters<T>: Send + Sync + 'static,
{
}

/// Something which knows how to convert itself into a signal.
///
/// You actually build signals up with these, not with the signal traits directly.
///
/// Again, this trait is in practice sealed.
pub trait IntoSignal {
    type Signal: Signal;

    fn into_signal(self) -> Result<Self::Signal>;
}

// Workarounds for https://github.com/rust-lang/rust/issues/38078: rustc is not always able to determine when a type
// isn't ambiguous, or at the very least it doesn't tell us what the options are, so we use this instead.
pub(crate) type IntoSignalOutput<S> = <<S as IntoSignal>::Signal as Signal>::Output;
pub(crate) type IntoSignalInput<S> = <<S as IntoSignal>::Signal as Signal>::Input;
pub(crate) type IntoSignalParameters<S> = <<S as IntoSignal>::Signal as Signal>::Parameters;
pub(crate) type IntoSignalState<S> = <<S as IntoSignal>::Signal as Signal>::State;
pub(crate) type SignalSealedInput<T> = <T as Signal>::Input;
pub(crate) type SignalSealedOutput<T> = <T as Signal>::Output;
pub(crate) type SignalSealedState<S> = <S as Signal>::State;
pub(crate) type SignalSealedParameters<S> = <S as Signal>::Parameters;

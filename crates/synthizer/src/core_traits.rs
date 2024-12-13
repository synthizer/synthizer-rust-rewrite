use crate::error::Result;

/// A signal.
///
/// This is a lot like an iterator, but instead of pulling values out, values are pushed along instead.  This allows for
/// "forking" in the middle, e.g. for sidechains and bypasses.
///
/// All signals have:
///
/// - A state, which will be materialized on the audio thread.
/// - Some parameters, which are read-only on the audio thread (but interior mutability is allowed).
/// - An input type, which is what the signal processes.
/// - An output type, which is what the signal produces.
///
/// Users external to this crate are not expected to implement this trait directly, and in fact doing so is impossible
/// because many of the arguments have private fields.  Instead, you should build signals up from the smaller pieces
/// this crate provides.
///
/// Note: this trait is primarily implemented on zero-sized types which describe a control flow graph.  The entrypoint
/// to the crate is `SignalBuilder`, which builds signals using types that also contain settings before converting them
/// to their equivalents and pre-allocating various pieces of machinery.  Note that being ZST is *not* guaranteed.
pub trait Signal: Sized {
    type Input: Sized;
    type Output: Sized;
    type State: Sized;
    type Parameters: Sized;

    /// Tick this signal once.
    fn tick1<D: SignalDestination<Self::Output>>(
        ctx: &mut SignalExecutionContext<'_, Self::State, Self::Parameters>,
        input: &'_ Self::Input,
        destination: D,
    );
}

pub trait SignalDestination<Input: Sized> {
    fn send(&mut self, value: Input);
}

impl<F, Input> SignalDestination<Input> for F
where
    Input: Sized,

    F: FnMut(Input),
{
    fn send(&mut self, value: Input) {
        (*self)(value)
    }
}

pub struct SignalExecutionContext<'a, TState, TParameters> {
    pub(crate) state: &'a mut TState,
    pub(crate) parameters: &'a TParameters,

    /// Time in samples from some epoch.
    pub(crate) time: u64,
}

impl<TState, TParameters> SignalExecutionContext<'_, TState, TParameters> {
    /// Convert this context into values usually derived from reborrows of this context's fields.  Used to grab parts of
    /// contexts when moving upstream.
    pub(crate) fn wrap<'a, NewS, NewP>(
        &'a mut self,
        new_state: impl FnOnce(&'a mut TState) -> &'a mut NewS,
        new_params: impl FnOnce(&'a TParameters) -> &'a NewP,
    ) -> SignalExecutionContext<'a, NewS, NewP> {
        SignalExecutionContext {
            state: new_state(self.state),
            parameters: new_params(self.parameters),
            time: self.time,
        }
    }
}

/// A helper trait to write bounds over signals which do not have inputs.
///
/// These are special, because they can be mounted into the audio thread.  This generally means that the input is coming
/// from a signal which knows how to produce values "out of thin air", either by performing mathematics or by getting
/// data through some other mechanism.
pub trait Generator: Signal<Input = ()> {}
impl<T> Generator for T where T: Signal<Input = ()> {}

/// A mountable signal has no inputs and no outputs.
pub trait Mountable: Generator + Signal<Output = ()> {}
impl<T> Mountable for T where T: Generator + Signal<Output = ()> {}

/// Something which knows how to convert itself into a signal.
///
/// You actually build signals up with these, not with the signal traits directly.
///
/// Again, this trait is in practice sealed.
pub trait IntoSignal {
    type Signal: Signal;

    fn into_signal(self) -> Result<Self::Signal>;
}

/// Workaround for https://github.com/rust-lang/rust/issues/38078: rustc is not always able to determine when a type
/// isn't ambiguous, or at the very least it doesn't tell us what the options are, so we use this instead.
#[allow(type_alias_bounds)]
pub type IntoSignalOutput<S: IntoSignal> = <S::Signal as Signal>::Output;

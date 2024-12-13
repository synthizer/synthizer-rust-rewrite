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
/// - A command type, which is what the signal may use to communicate with signals upstream of it.
/// - A metadata type, which lets signals communicate with signals downstream.
pub trait Signal: Sized {
    type Input: Sized;
    type Output: Sized;
    type Command: Sized;
    type Metadata: Sized;
    type State: Sized;
    type Parameters: Sized;

    /// Produce metadata, if any is available.
    ///
    /// Default implementation does not produce metadata.
    fn produce_metadata(_ctx: SignalExecutionContext<'_, Self>) -> Option<Self::Metadata> {
        None
    }

    /// Consume a command, doing whatever is appropriate to the state.
    ///
    /// Default drops commands on the floor.
    ///
    /// This will be called after a tick of a downstream signal which wishes to send a command, as the return value of
    /// said downstream's destination callback.  Wrapper signals must show the command to their parents.
    fn consume_command(_ctx: &mut SignalExecutionContext<'_, Self>, command: &Self::Command) {}

    /// Tick this signal once.
    fn tick1<D: SignalDestination<Self::Output, Command = Self::Command>>(
        ctx: &SignalExecutionContext<'_, Self>,
        input: &'_ Self::Input,
        destination: D,
    );
}

trait SignalDestination<Input: Sized> {
    type Command: Sized;

    fn send(&mut self, value: Input) -> Option<Self::Command>;
}

impl<F, Input, Cmd> SignalDestination<Input> for F
where
    Input: Sized,
    Cmd: Sized,
    F: FnMut(Input) -> Option<Cmd>,
{
    type Command = Cmd;

    fn send(&mut self, value: Input) -> Option<Self::Command> {
        (*self)(value)
    }
}
pub struct SignalExecutionContext<'a, S: Signal> {
    state: &'a mut S::State,
    parameters: &'a S::Parameters,
}

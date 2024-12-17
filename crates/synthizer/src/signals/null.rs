use crate::context::*;
use crate::core_traits::*;

/// The null signal: a signal which takes nothing and outputs nothing.
///
/// We have to start chains somehow.  This is the "empty chain" signal: it can be mounted but if it is, it does nothing
/// at all.
pub struct NullSignal(());

unsafe impl Signal for NullSignal {
    type Input = ();
    type Output = ();
    type State = ();
    type Parameters = ();

    fn tick1<D: SignalDestination<Self::Output>>(
        _ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        _input: &'_ Self::Input,
        destination: D,
    ) {
        destination.send(());
    }

    fn on_block_start(_ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>) {}
}

impl IntoSignal for NullSignal {
    type Signal = NullSignal;

    fn into_signal(
        self,
    ) -> crate::Result<ReadySignal<Self::Signal, IntoSignalState<Self>, IntoSignalParameters<Self>>>
    {
        Ok(ReadySignal {
            signal: NullSignal::new(),
            state: (),
            parameters: (),
        })
    }
}

impl NullSignal {
    pub(crate) fn new() -> NullSignal {
        NullSignal(())
    }
}

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

    fn tick_block<
        'a,
        I: FnMut(usize) -> &'a Self::Input,
        D: ReusableSignalDestination<Self::Output>,
    >(
        _ctx: &'_ mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        mut input: I,
        mut destination: D,
    ) where
        Self::Input: 'a,
    {
        for i in 0..crate::config::BLOCK_SIZE {
            input(i);
            destination.send_reusable(());
        }
    }

    fn on_block_start(_ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>) {}

    fn trace_slots<
        F: FnMut(
            crate::unique_id::UniqueId,
            std::sync::Arc<dyn std::any::Any + Send + Sync + 'static>,
        ),
    >(
        _state: &Self::State,
        _parameters: &Self::Parameters,
        _inserter: &mut F,
    ) {
    }
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

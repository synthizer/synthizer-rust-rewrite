use crate::context::*;
use crate::core_traits::*;

/// The null signal: a signal which takes nothing and outputs nothing.
///
/// We have to start chains somehow.  This is the "empty chain" signal: it can be mounted but if it is, it does nothing
/// at all.
pub struct NullSignal(());

unsafe impl Signal for NullSignal {
    type Input<'il> = ();
    type Output<'ol> = ();
    type State = ();
    type Parameters = ();

    fn tick<'il, 'ol, D, const N: usize>(
        _ctx: &'_ SignalExecutionContext<'_, '_>,
        _input: [Self::Input<'il>; N],
        _params: &Self::Parameters,
        _state: &mut Self::State,
        destination: D,
    ) where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        D: SignalDestination<Self::Output<'ol>, N>,
    {
        destination.send([(); N]);
    }

    fn on_block_start(
        _ctx: &SignalExecutionContext<'_, '_>,
        _params: &Self::Parameters,
        _state: &mut Self::State,
    ) {
    }

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

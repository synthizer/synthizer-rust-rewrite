use std::marker::PhantomData as PD;

use crate::core_traits::*;
use crate::error::Result;

pub struct MapSignal<ParSig, F, O>(PD<*const (ParSig, F, O)>);
unsafe impl<ParSig, F, O> Send for MapSignal<ParSig, F, O> {}
unsafe impl<ParSig, F, O> Sync for MapSignal<ParSig, F, O> {}

pub struct MapSignalConfig<ParSigCfg, F, O> {
    parent: ParSigCfg,
    closure: F,
    _phantom: PD<O>,
}

pub struct MapSignalState<ParSig: Signal, F> {
    closure: F,
    parent_state: SignalState<ParSig>,
}

unsafe impl<ParSig, F, O> Signal for MapSignal<ParSig, F, O>
where
    ParSig: Signal,
    F: FnMut(SignalOutput<ParSig>) -> O + Send + Sync + 'static,
    O: Send + 'static,
{
    type Input = SignalInput<ParSig>;
    type Output = O;
    type State = MapSignalState<ParSig, F>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        ParSig::on_block_start(ctx, &mut state.parent_state);
    }

    fn tick_frame(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let parent_output = ParSig::tick_frame(ctx, input, &mut state.parent_state);
        (state.closure)(parent_output)
    }
}

impl<ParSig, F, O> IntoSignal for MapSignalConfig<ParSig, F, O>
where
    F: FnMut(IntoSignalOutput<ParSig>) -> O + Send + Sync + 'static,
    ParSig: IntoSignal,
    O: Send + 'static,
{
    type Signal = MapSignal<ParSig::Signal, F, O>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let par = self.parent.into_signal()?;

        Ok(ReadySignal {
            state: MapSignalState {
                closure: self.closure,
                parent_state: par.state,
            },
            signal: MapSignal(PD),
        })
    }

    fn trace<Tracer: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        inserter: &mut Tracer,
    ) -> Result<()> {
        self.parent.trace(inserter)?;
        Ok(())
    }
}

impl<ParSig, F, O> MapSignalConfig<ParSig, F, O> {
    pub(crate) fn new(parent: ParSig, closure: F) -> Self {
        Self {
            closure,
            parent,
            _phantom: PD,
        }
    }
}

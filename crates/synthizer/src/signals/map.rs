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
    SignalOutput<ParSig>: Clone,
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

    fn tick<I, const N: usize>(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: I,
        state: &mut Self::State,
    ) -> impl ValueProvider<Self::Output>
    where
        I: ValueProvider<Self::Input> + Sized,
    {
        let parent = ParSig::tick::<_, N>(ctx, input, &mut state.parent_state);
        let mapped = parent.iter_cloned().map(&mut state.closure);

        ArrayProvider::<_, N>::new(crate::array_utils::collect_iter(mapped))
    }
}

impl<ParSig, F, O> IntoSignal for MapSignalConfig<ParSig, F, O>
where
    F: FnMut(IntoSignalOutput<ParSig>) -> O + Send + Sync + 'static,
    ParSig: IntoSignal,
    O: Send + 'static,
    IntoSignalOutput<ParSig>: Clone,
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

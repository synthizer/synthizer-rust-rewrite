use std::marker::PhantomData as PD;

use crate::core_traits::*;

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
    type Input<'il> = SignalInput<'il, ParSig>;
    type Output<'ol> = O;
    type State = MapSignalState<ParSig, F>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        ParSig::on_block_start(ctx, &mut state.parent_state);
    }

    fn tick<'il, 'ol, I, const N: usize>(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: I,
        state: &mut Self::State,
    ) -> impl ValueProvider<Self::Output<'ol>>
    where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        I: ValueProvider<Self::Input<'il>> + Sized,
    {
        let parent = ParSig::tick::<_, N>(ctx, input, &mut state.parent_state);
        let mapped = unsafe { parent.become_iterator() }.map(&mut state.closure);

        ArrayProvider::<_, N>::new(crate::array_utils::collect_iter(mapped))
    }

    fn trace_slots<
        Tracer: FnMut(
            crate::unique_id::UniqueId,
            std::sync::Arc<dyn std::any::Any + Send + Sync + 'static>,
        ),
    >(
        state: &Self::State,
        inserter: &mut Tracer,
    ) {
        ParSig::trace_slots(&state.parent_state, inserter);
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

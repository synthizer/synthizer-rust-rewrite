use std::marker::PhantomData as PD;

use crate::core_traits::*;

pub struct MapInputSignal<ParSig, F, I>(PD<*const (ParSig, F, I)>);
unsafe impl<ParSig, F, I> Send for MapInputSignal<ParSig, F, I> {}
unsafe impl<ParSig, F, I> Sync for MapInputSignal<ParSig, F, I> {}

pub struct MapInputSignalConfig<ParSigCfg, F, I> {
    parent: ParSigCfg,
    closure: F,
    _phantom: PD<I>,
}

pub struct MapInputSignalState<ParSig: Signal, F> {
    closure: F,
    parent_state: SignalState<ParSig>,
}

unsafe impl<ParSig, F, I, IResult> Signal for MapInputSignal<ParSig, F, I>
where
    ParSig: Signal<Input = IResult>,
    F: FnMut(I) -> IResult + Send + Sync + 'static,
    I: Send + 'static,
    IResult: 'static,
{
    type Input = I;
    type Output = ParSig::Output;
    type State = MapInputSignalState<ParSig, F>;

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
        let mapped_input = (state.closure)(input);
        ParSig::tick_frame(ctx, mapped_input, &mut state.parent_state)
    }
}

impl<ParSig, F, I> IntoSignal for MapInputSignalConfig<ParSig, F, I>
where
    ParSig: IntoSignal,
    MapInputSignal<ParSig::Signal, F, I>: Signal<State = MapInputSignalState<ParSig::Signal, F>>,
{
    type Signal = MapInputSignal<ParSig::Signal, F, I>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let par = self.parent.into_signal()?;

        Ok(ReadySignal {
            state: MapInputSignalState {
                closure: self.closure,
                parent_state: par.state,
            },
            signal: MapInputSignal(PD),
        })
    }
}

impl<ParSig, F, O> MapInputSignalConfig<ParSig, F, O> {
    pub(crate) fn new(parent: ParSig, closure: F) -> Self {
        Self {
            closure,
            parent,
            _phantom: PD,
        }
    }
}

use crate::context::*;
use crate::core_traits::*;

pub struct SinSignalConfig<S: IntoSignal> {
    pub(crate) wrapped: S,
}

pub struct SinSignal<S>(S);

unsafe impl<S> Signal for SinSignal<S>
where
    S: for<'ol> Signal<Output<'ol> = f64>,
{
    type Input<'il> = S::Input<'il>;
    type Output<'ol> = S::Output<'ol>;
    type State = S::State;
    type Parameters = S::Parameters;

    fn on_block_start(
        ctx: &SignalExecutionContext<'_, '_>,
        params: &Self::Parameters,
        state: &mut Self::State,
    ) {
        S::on_block_start(ctx, params, state);
    }

    fn tick<'il, 'ol, D, const N: usize>(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: [Self::Input<'il>; N],
        params: &Self::Parameters,
        state: &mut Self::State,
        destination: D,
    ) where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        D: SignalDestination<Self::Output<'ol>, N>,
    {
        S::tick::<_, N>(ctx, input, params, state, |x: [f64; N]| {
            destination.send(x.map(|x| x.sin()))
        });
    }

    fn trace_slots<
        F: FnMut(
            crate::unique_id::UniqueId,
            std::sync::Arc<dyn std::any::Any + Send + Sync + 'static>,
        ),
    >(
        state: &Self::State,
        parameters: &Self::Parameters,
        inserter: &mut F,
    ) {
        S::trace_slots(state, parameters, inserter);
    }
}

impl<S> IntoSignal for SinSignalConfig<S>
where
    S: IntoSignal,
    for<'a> S::Signal: Signal<Output<'a> = f64>,
{
    type Signal = SinSignal<S::Signal>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let wrapped = self.wrapped.into_signal()?;
        Ok(ReadySignal {
            signal: SinSignal(wrapped.signal),
            state: wrapped.state,
            parameters: wrapped.parameters,
        })
    }
}

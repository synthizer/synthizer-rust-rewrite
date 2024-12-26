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
    type Output<'ol> = f64;
    type State = S::State;

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        S::on_block_start(ctx, state);
    }

    fn tick<'il, 'ol, I, const N: usize>(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: I,
        state: &mut Self::State,
    ) -> impl ValueProvider<f64>
    where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        I: ValueProvider<Self::Input<'il>> + Sized,
    {
        let mut par_provider = S::tick::<_, N>(ctx, input, state);
        ClosureProvider::<_, _, N>::new(move |index| par_provider.get(index).sin())
    }

    fn trace_slots<
        F: FnMut(
            crate::unique_id::UniqueId,
            std::sync::Arc<dyn std::any::Any + Send + Sync + 'static>,
        ),
    >(
        state: &Self::State,
        inserter: &mut F,
    ) {
        S::trace_slots(state, inserter);
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
        })
    }
}

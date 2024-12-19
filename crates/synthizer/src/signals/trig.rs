use crate::context::*;
use crate::core_traits::*;

pub struct SinSignalConfig<S: IntoSignal> {
    pub(crate) wrapped: S,
}

pub struct SinSignal<S>(S);

unsafe impl<S> Signal for SinSignal<S>
where
    S: Signal<Output = f64>,
{
    type Input = S::Input;
    type Output = S::Output;
    type State = S::State;
    type Parameters = S::Parameters;

    fn on_block_start(ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>) {
        S::on_block_start(ctx);
    }

    fn tick<
        'a,
        I: FnMut(usize) -> &'a Self::Input,
        D: ReusableSignalDestination<Self::Output>,
        const N: usize,
    >(
        ctx: &'_ mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        input: I,
        mut destination: D,
    ) where
        Self::Input: 'a,
    {
        S::tick::<_, _, N>(ctx, input, |x: f64| destination.send_reusable(x.sin()));
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
    S::Signal: Signal<Output = f64>,
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

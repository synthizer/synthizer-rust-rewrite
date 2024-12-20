use std::marker::PhantomData as PD;

use crate::context::*;
use crate::core_traits::*;

/// Consume the input of this signal.  Then replace it with the `Default::default()` value of a new input type.
///
/// This is basically a no-op signal.  It is useful to get from signals which take `()` as input to signals which take
/// some other type of input so that they may be lifted up and joined into other signals using mathematical operators.
/// Because Rust does not have specialization, we cannot write a version of the mathematical traits which understands
/// that signals whose input are `()` may take any input, and concrete casting is sadly required.
pub(crate) struct ConsumeInputSignal<Wrapped, DiscardingInputType>(
    Wrapped,
    PD<DiscardingInputType>,
);

pub(crate) struct ConsumeInputSignalConfig<Wrapped, DiscardingInputType>(
    Wrapped,
    PD<DiscardingInputType>,
);

unsafe impl<S, I> Send for ConsumeInputSignal<S, I> where S: Send {}
unsafe impl<S, I> Sync for ConsumeInputSignal<S, I> where S: Sync {}

unsafe impl<S, I> Signal for ConsumeInputSignal<S, I>
where
    S: Signal + 'static,
    for<'a> S::Input<'a>: Default,
{
    type Input<'il> = I;
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
        _input: [Self::Input<'il>; N],
        params: &Self::Parameters,
        state: &mut Self::State,
        destination: D,
    ) where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        D: SignalDestination<Self::Output<'ol>, N>,
    {
        let ni = [(); N].map(|_| Default::default());
        S::tick::<_, N>(ctx, ni, params, state, destination);
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

impl<S, DiscardingInputType> IntoSignal for ConsumeInputSignalConfig<S, DiscardingInputType>
where
    S: IntoSignal,
    for<'a> IntoSignalInput<'a, S>: Default,
    S::Signal: 'static,
{
    type Signal = ConsumeInputSignal<S::Signal, DiscardingInputType>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let inner = self.0.into_signal()?;

        Ok(ReadySignal {
            signal: ConsumeInputSignal(inner.signal, PD),
            state: inner.state,
            parameters: inner.parameters,
        })
    }
}

impl<S, DiscardingInputType> ConsumeInputSignalConfig<S, DiscardingInputType> {
    pub(crate) fn new(wrapped: S) -> Self {
        Self(wrapped, PD)
    }
}

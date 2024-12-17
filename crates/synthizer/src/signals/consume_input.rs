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
    S: Signal,
    S::Input: Default,
{
    type Input = I;
    type Output = S::Output;
    type State = S::State;
    type Parameters = S::Parameters;

    fn tick1<D: SignalDestination<Self::Output>>(
        ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        _input: &'_ Self::Input,
        destination: D,
    ) {
        let new_in: S::Input = Default::default();
        S::tick1(ctx, &new_in, destination);
    }

    fn on_block_start(ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>) {
        S::on_block_start(ctx);
    }
}

impl<S, DiscardingInputType> IntoSignal for ConsumeInputSignalConfig<S, DiscardingInputType>
where
    S: IntoSignal,
    IntoSignalInput<S>: Default,
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

use std::marker::PhantomData as PD;

use crate::context::*;
use crate::core_traits::*;
use crate::error::Result;

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

unsafe impl<S, OldInputTy> Signal for ConsumeInputSignal<S, OldInputTy>
where
    S: Signal + 'static,
    S::Input: Default,
    OldInputTy: 'static,
{
    type Input = OldInputTy;
    type Output = S::Output;
    type State = S::State;

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        S::on_block_start(ctx, state);
    }

    fn tick<I, const N: usize>(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        _input: I,
        state: &mut Self::State,
    ) -> impl ValueProvider<Self::Output>
    where
        I: ValueProvider<Self::Input> + Sized,
    {
        S::tick::<_, N>(
            ctx,
            ClosureProvider::<_, _, N>::new(|_| Default::default()),
            state,
        )
    }
}

impl<S, DiscardingInputType> IntoSignal for ConsumeInputSignalConfig<S, DiscardingInputType>
where
    S: IntoSignal,
    IntoSignalInput<S>: Default,
    S::Signal: 'static,
    DiscardingInputType: 'static,
{
    type Signal = ConsumeInputSignal<S::Signal, DiscardingInputType>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let inner = self.0.into_signal()?;

        Ok(ReadySignal {
            signal: ConsumeInputSignal(inner.signal, PD),
            state: inner.state,
        })
    }

    fn trace<F: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        inserter: &mut F,
    ) -> Result<()> {
        self.0.trace(inserter)?;
        Ok(())
    }
}

impl<S, DiscardingInputType> ConsumeInputSignalConfig<S, DiscardingInputType> {
    pub(crate) fn new(wrapped: S) -> Self {
        Self(wrapped, PD)
    }
}

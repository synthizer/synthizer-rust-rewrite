use crate::context::*;
use crate::core_traits::*;
use crate::error::Result;

pub struct SinSignalConfig<S: IntoSignal> {
    pub(crate) wrapped: S,
}

pub struct SinSignal<S>(S);

unsafe impl<S> Signal for SinSignal<S>
where
    S: Signal<Output = f64>,
{
    type Input = S::Input;
    type Output = f64;
    type State = S::State;

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        S::on_block_start(ctx, state);
    }

    fn tick<I, const N: usize>(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: I,
        state: &mut Self::State,
    ) -> impl ValueProvider<f64>
    where
        I: ValueProvider<Self::Input> + Sized,
    {
        let mut par_provider = S::tick::<_, N>(ctx, input, state);
        ClosureProvider::<_, _, N>::new(move |index| par_provider.get(index).sin())
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
        })
    }

    fn trace<F: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        inserter: &mut F,
    ) -> Result<()> {
        self.wrapped.trace(inserter)?;
        Ok(())
    }
}

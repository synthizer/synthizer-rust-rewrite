use std::marker::PhantomData as PD;

use crate::core_traits::*;

pub struct SinSignalConfig<S: IntoSignal> {
    pub(crate) wrapped: S,
}

pub struct SinSignal<S>(PD<*mut S>);

impl<S> Signal for SinSignal<S>
where
    S: Signal<Output = f64>,
{
    type Input = S::Input;
    type Output = S::Output;
    type State = S::State;
    type Parameters = S::Parameters;

    fn tick1<D: SignalDestination<Self::Output>>(
        ctx: &mut SignalExecutionContext<'_, Self::State, Self::Parameters>,
        input: &'_ Self::Input,
        mut destination: D,
    ) {
        S::tick1(ctx, input, |x: f64| destination.send(x.sin()));
    }
}

impl<S> IntoSignal for SinSignalConfig<S>
where
    S: IntoSignal,
    S::Signal: Signal<Output = f64>,
{
    type Signal = SinSignal<S::Signal>;

    fn into_signal(self) -> crate::Result<Self::Signal> {
        let _wrapped = self.wrapped.into_signal()?;
        Ok(SinSignal(PD))
    }
}

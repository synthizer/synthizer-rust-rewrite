use crate::context::*;
use crate::core_traits::*;

pub struct AudioOutputSignal<S>(S);
pub struct AudioOutputSignalConfig<S>(S);

unsafe impl<S> Signal for AudioOutputSignal<S>
where
    S: Signal<Output = f64>,
{
    type Output = ();
    type Input = S::Input;
    type State = S::State;
    type Parameters = S::Parameters;

    fn tick1<D: SignalDestination<Self::Output>>(
        ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        input: &'_ Self::Input,
        destination: D,
    ) {
        let mut val: Option<S::Output> = None;
        S::tick1(ctx, input, |x| val = Some(x));

        // We output the unit type instead.
        destination.send(());

        // TODO: this is actually going to the audio output buffer.
    }
}

impl<S> IntoSignal for AudioOutputSignalConfig<S>
where
    S: IntoSignal,
    S::Signal: Signal<Output = f64>,
{
    type Signal = AudioOutputSignal<S::Signal>;

    fn into_signal(self) -> crate::Result<Self::Signal> {
        Ok(AudioOutputSignal(self.0.into_signal()?))
    }
}

impl<S> AudioOutputSignalConfig<S> {
    pub(crate) fn new(signal: S) -> Self {
        Self(signal)
    }
}

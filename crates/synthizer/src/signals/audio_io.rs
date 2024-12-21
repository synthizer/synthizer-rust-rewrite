use crate::context::*;
use crate::core_traits::*;

pub struct AudioOutputSignal<S>(S);
pub struct AudioOutputSignalConfig<S>(S);

unsafe impl<S> Signal for AudioOutputSignal<S>
where
    for<'a> S: Signal<Output<'a> = f64>,
{
    type Output<'ol> = ();
    type Input<'il> = S::Input<'il>;
    // The parent state, with a usize tacked on for tick1's counter.
    type State = (S::State, usize);
    type Parameters = S::Parameters;

    fn on_block_start(
        ctx: &SignalExecutionContext<'_, '_>,
        params: &Self::Parameters,
        state: &mut Self::State,
    ) {
        S::on_block_start(ctx, params, &mut state.0);
        state.1 = 0;
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
        let mut block = None;
        S::tick::<_, N>(ctx, input, params, &mut state.0, |x: [f64; N]| {
            block = Some(x);
        });
        let block = block.unwrap();
        {
            let mut dest = ctx.fixed.audio_destinationh.borrow_mut();
            dest[state.1..(state.1 + N)].copy_from_slice(&block);
            state.1 += N;
        }

        destination.send([(); N]);
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
        S::trace_slots(&state.0, parameters, inserter);
    }
}

impl<S> IntoSignal for AudioOutputSignalConfig<S>
where
    S: IntoSignal,
    for<'ol> S::Signal: Signal<Output<'ol> = f64>,
{
    type Signal = AudioOutputSignal<S::Signal>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let inner = self.0.into_signal()?;
        Ok(ReadySignal {
            signal: AudioOutputSignal(inner.signal),
            state: (inner.state, 0),
            parameters: inner.parameters,
        })
    }
}

impl<S> AudioOutputSignalConfig<S> {
    pub(crate) fn new(signal: S) -> Self {
        Self(signal)
    }
}

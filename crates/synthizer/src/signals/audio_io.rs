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

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        S::on_block_start(ctx, &mut state.0);
        state.1 = 0;
    }

    fn tick<'il, 'ol, D, const N: usize>(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: [Self::Input<'il>; N],
        state: &mut Self::State,
        destination: D,
    ) where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        D: SignalDestination<Self::Output<'ol>, N>,
    {
        let mut block = None;
        S::tick::<_, N>(ctx, input, &mut state.0, |x: [f64; N]| {
            block = Some(x);
        });
        let mut block = block.unwrap();
        let mut temp = [[0.0f64; 2]; N];
        crate::channel_conversion::convert_channels(
            &crate::audio_frames::DefaultingFrameWrapper::wrap_array(&mut block),
            crate::ChannelFormat::Mono,
            &mut temp,
            crate::ChannelFormat::Stereo,
        );
        {
            let mut dest = ctx.fixed.audio_destinationh.borrow_mut();
            dest[state.1..(state.1 + N)].copy_from_slice(&temp);
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
        inserter: &mut F,
    ) {
        S::trace_slots(&state.0, inserter);
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
        })
    }
}

impl<S> AudioOutputSignalConfig<S> {
    pub(crate) fn new(signal: S) -> Self {
        Self(signal)
    }
}

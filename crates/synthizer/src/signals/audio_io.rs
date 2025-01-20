use crate::channel_format::ChannelFormat;
use crate::context::*;
use crate::core_traits::*;
use crate::error::Result;

pub struct AudioOutputSignal<S>(S);
pub struct AudioOutputSignalConfig<S> {
    parent_cfg: S,
    format: ChannelFormat,
}

pub struct AudioOutputState<T> {
    offset: usize,
    format: crate::ChannelFormat,
    underlying_state: T,
}

unsafe impl<S> Signal for AudioOutputSignal<S>
where
    for<'a> S: Signal,
    SignalOutput<S>: AudioFrame<f64> + Clone,
{
    type Output = ();
    type Input = S::Input;
    type State = AudioOutputState<S::State>;

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        S::on_block_start(ctx, &mut state.underlying_state);
        state.offset = 0;
    }

    fn tick<I, const N: usize>(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: I,
        state: &mut Self::State,
    ) -> impl ValueProvider<()>
    where
        I: ValueProvider<Self::Input> + Sized,
    {
        let block = crate::array_utils::collect_iter::<_, N>(
            S::tick::<_, N>(ctx, input, &mut state.underlying_state).iter_cloned(),
        );

        let mut temp = [[0.0f64; 2]; N];
        crate::channel_conversion::convert_channels(
            &block,
            state.format,
            &mut temp,
            crate::ChannelFormat::Stereo,
        );
        {
            let mut dest = ctx.fixed.audio_destinationh.borrow_mut();
            dest[state.offset..(state.offset + N)].copy_from_slice(&temp);
            state.offset += N;
        }

        FixedValueProvider::<_, N>::new(())
    }
}

impl<S> IntoSignal for AudioOutputSignalConfig<S>
where
    S: IntoSignal,
    IntoSignalOutput<S>: AudioFrame<f64> + Clone,
{
    type Signal = AudioOutputSignal<S::Signal>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let inner = self.parent_cfg.into_signal()?;
        Ok(ReadySignal {
            signal: AudioOutputSignal(inner.signal),
            state: AudioOutputState {
                offset: 0,
                underlying_state: inner.state,
                format: self.format,
            },
        })
    }

    fn trace<F: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        inserter: &mut F,
    ) -> Result<()> {
        self.parent_cfg.trace(inserter)?;
        Ok(())
    }
}

impl<S> AudioOutputSignalConfig<S> {
    pub(crate) fn new(signal: S, format: ChannelFormat) -> Self {
        Self {
            parent_cfg: signal,
            format,
        }
    }
}

use crate::channel_format::ChannelFormat;
use crate::context::*;
use crate::core_traits::*;

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
    SignalOutput<S>: AudioFrame<f64>,
{
    type Output = ();
    type Input = S::Input;
    type State = AudioOutputState<S::State>;

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        S::on_block_start(ctx, &mut state.underlying_state);
        state.offset = 0;
    }

    fn tick_frame(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let frame = S::tick_frame(ctx, input, &mut state.underlying_state);

        // Convert single frame using the convert_channels function
        let input_array = [frame];
        let mut output_array = [[0.0f64; 2]];
        crate::channel_conversion::convert_channels(
            &input_array,
            state.format,
            &mut output_array,
            crate::ChannelFormat::Stereo,
        );

        {
            let mut dest = ctx.fixed.audio_destinationh.borrow_mut();
            dest[state.offset] = output_array[0];
            state.offset += 1;
        }
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
}

impl<S> AudioOutputSignalConfig<S> {
    pub(crate) fn new(signal: S, format: ChannelFormat) -> Self {
        Self {
            parent_cfg: signal,
            format,
        }
    }
}

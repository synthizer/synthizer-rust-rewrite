use crate::channel_format::ChannelFormat;
use crate::core_traits::*;

/// Mono to anything.
fn broadcast<I, O, const N: usize>(input: &[I; N], output: &mut [O; N])
where
    I: AudioFrame<f64>,
    O: AudioFrame<f64>,
{
    for (iframe, oframe) in input.iter().zip(output.iter_mut()) {
        for j in 0..oframe.channel_count() {
            oframe.set(j, iframe.get_or_default(0));
        }
    }
}

fn to_mono<I, O, const N: usize>(input: &[I; N], output: &mut [O; N])
where
    I: AudioFrame<f64>,
    O: AudioFrame<f64>,
{
    for (input, output) in input.iter().zip(output.iter_mut()) {
        let mut sum = 0.0f64;
        for i in 0..input.channel_count() {
            sum += input.get_or_default(i);
        }
        sum /= input.channel_count() as f64;
        output.set(0, sum);
    }
}

fn expand_or_truncate<I, O, const N: usize>(input: &[I; N], output: &mut [O; N])
where
    I: AudioFrame<f64>,
    O: AudioFrame<f64>,
{
    for (input, output) in input.iter().zip(output.iter_mut()) {
        for i in 0..input.channel_count() {
            output.set_or_ignore(i, *input.get(i));
        }
    }
}

pub(crate) fn convert_channels<I, O, const N: usize>(
    input: &[I; N],
    input_format: ChannelFormat,
    output: &mut [O; N],
    output_format: ChannelFormat,
) where
    I: AudioFrame<f64>,
    O: AudioFrame<f64>,
{
    match (input_format, output_format) {
        (ChannelFormat::Mono, _) => broadcast(input, output),
        (_, ChannelFormat::Mono) => to_mono(input, output),
        (_, _) => expand_or_truncate(input, output),
    }
}

use crate::ChannelFormat;
use crate::OutputBuffer;

struct ConversionArgs<'a, OB> {
    input_format: &'a ChannelFormat,
    output_format: &'a ChannelFormat,
    input_data: &'a [f32],
    output_buffer: &'a mut OB,
}

/// A converter which can convert between two channel formats.
///
/// The rules are as follows:
///
/// - Mono to stereo broadcasts the mono channel equally to both stereo channels.
/// - Stereo to mono squashes the stereo channels together.
/// - Any raw format to another raw format with more channels sets the additional channels to 0.
/// - Any raw format to a raw format with less channels truncates the additional channels.
/// - Any raw format to and/or from anything else is an error.
pub struct ChannelConverter {
    input_format: ChannelFormat,
    output_format: ChannelFormat,
}

/// Reasons it isn't possible to convert from one format to another.
#[derive(Debug, thiserror::Error)]
pub enum ChannelConversionError {
    #[error("The input format is raw, but the output format isn't")]
    OnlyInputRaw,

    #[error("The output format is raw, but the input isn't")]
    OnlyOutputRaw,
}

impl ChannelConverter {
    /// get a converter to convert from the input to output types, if possible.
    pub fn new(
        input_format: ChannelFormat,
        output_format: ChannelFormat,
    ) -> Result<ChannelConverter, ChannelConversionError> {
        use ChannelFormat as Ch;

        match (&input_format, &output_format) {
            (Ch::Raw { .. }, x) if !x.is_raw() => return Err(ChannelConversionError::OnlyInputRaw),
            (x, Ch::Raw { .. }) if !x.is_raw() => {
                return Err(ChannelConversionError::OnlyOutputRaw)
            }
            _ => (),
        }

        Ok(ChannelConverter {
            input_format,
            output_format,
        })
    }

    /// Convert some data from the flat, interleaved input and write it to the given output buffer.
    ///
    /// The input data must be a multiple of the channel count of the input format.
    #[inline(always)]
    pub fn convert<OB: OutputBuffer<SampleType = f32>>(
        &self,
        input_data: &[f32],
        output_buffer: &mut OB,
    ) {
        use ChannelFormat as CF;

        let mut args = ConversionArgs {
            input_format: &self.input_format,
            output_format: &self.output_format,
            input_data,
            output_buffer,
        };

        match (&self.input_format, &self.output_format) {
            (CF::Mono, CF::Stereo) => mono_to_stereo(&mut args),
            (CF::Stereo, CF::Mono) => stereo_to_mono(&mut args),
            (CF::Raw { .. }, CF::Raw { .. }) => raw_to_raw(&mut args),
            (x, y) if x.is_raw() ^ y.is_raw() => {
                panic!("The constructor should have errored for this case")
            }
            (_, _) => unreachable!(),
        }
    }
}

#[inline(always)]
fn mono_to_stereo<OB: OutputBuffer<SampleType = f32>>(args: &'_ mut ConversionArgs<'_, OB>) {
    for (i, s) in args.input_data.iter().enumerate() {
        let frame: [f32; 2] = [*s, *s];
        args.output_buffer.write_frame(i, &frame[..]);
    }
}

#[inline(always)]
fn stereo_to_mono<OB: OutputBuffer<SampleType = f32>>(args: &'_ mut ConversionArgs<'_, OB>) {
    for i in 0..args.input_data.len() / 2 {
        let left = args.input_data[i * 2];
        let right = args.input_data[i * 2 + 1];
        let frame = [(left + right) * 0.5f32];
        args.output_buffer.write_frame(i, &frame[..]);
    }
}

/// Convert raw to raw by either truncating or zeroing channels.
fn raw_to_raw<OB: OutputBuffer<SampleType = f32>>(args: &'_ mut ConversionArgs<'_, OB>) {
    let ichans = args.input_format.get_channel_count();
    let ochans = args.output_format.get_channel_count();
    let frames = args.input_data.len() / ichans;
    let frame_size = ichans.min(ochans).get();
    for f in 0..frames {
        let input_frame = &args.input_data[(ichans.get() * f)..];
        let will_write = &input_frame[0..frame_size];
        args.output_buffer.write_frame(f, will_write);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::output_buffer::SliceOutputBuffer;

    use std::num::NonZeroUsize;

    #[test]
    fn test_mono_to_stereo() {
        let input: [f32; 5] = [1.0, 2.0, 3.0, 4.0, 5.0];
        let mut output: [f32; 10] = [0.0; 10];

        let mixer = ChannelConverter::new(ChannelFormat::Mono, ChannelFormat::Stereo).unwrap();
        mixer.convert(
            &input[..],
            &mut SliceOutputBuffer::<f32, false>::new(&mut output[..], 2),
        );
        assert_eq!(output, [1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 5.0, 5.0]);
    }

    #[test]
    fn test_stereo_to_mono() {
        let input: [f32; 6] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let mut output: [f32; 3] = [0.0; 3];

        let converter = ChannelConverter::new(ChannelFormat::Stereo, ChannelFormat::Mono).unwrap();
        converter.convert(
            &input[..],
            &mut SliceOutputBuffer::<_, false>::new(&mut output[..], 1),
        );
        assert_eq!(output, [1.5, 3.5, 5.5]);
    }

    #[test]
    fn test_raw_truncation() {
        // 3 channels
        let input: [f32; 9] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];

        // 2 channels.
        let mut output: [f32; 6] = [0.0; 6];

        let converter = ChannelConverter::new(
            ChannelFormat::Raw {
                channels: NonZeroUsize::new(3).unwrap(),
            },
            ChannelFormat::Raw {
                channels: NonZeroUsize::new(2).unwrap(),
            },
        )
        .unwrap();

        converter.convert(
            &input[..],
            &mut SliceOutputBuffer::<_, false>::new(&mut output[..], 2),
        );
        assert_eq!(output, [1.0, 2.0, 4.0, 5.0, 7.0, 8.0]);
    }

    #[test]
    fn test_raw_zeroing() {
        // 2 channels
        let input: [f32; 6] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];

        // 3 channels.
        let mut output: [f32; 9] = [0.0; 9];

        let converter = ChannelConverter::new(
            ChannelFormat::Raw {
                channels: NonZeroUsize::new(2).unwrap(),
            },
            ChannelFormat::Raw {
                channels: NonZeroUsize::new(3).unwrap(),
            },
        )
        .unwrap();

        converter.convert(
            &input[..],
            &mut SliceOutputBuffer::<_, false>::new(&mut output[..], 2),
        );
        assert_eq!(output, [1.0, 2.0, 0.0, 3.0, 4.0, 0.0, 5.0, 6.0, 0.0]);
    }
}

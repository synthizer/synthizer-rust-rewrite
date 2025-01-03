use rubato::{Resampler as RubatoResampler, SincFixedOut, SincInterpolationParameters};

use crate::config::*;
use crate::data_structures::SplittableBuffer;
use crate::sample_sources::SampleSourceError;

use super::buffered::BufferedSourceReader;

/// A resampler wrapper that knows how to pull data through a backing implementation, currently Rubato, and yield it as
/// outputs.
///
/// Rubato has no understanding of proper partial endpoints where the input is finished, and requires flushing by
/// padding with zeros. It is also necessary to un-interleave input data for performance.  
///
/// Pending benchmarking, we may leave Rubato behind.
///
/// The output of this type is a single [crate::config::BLOCK_SIZE] block of data as an un-interleaved slice, `[c1, c1,
/// c1..., c2, c2, c2...]`. Streams do not currently support pitch bending, so that's fine.  Either this is running on
/// the audio thread, in which case the block is wanted directly, or in the background, in which case the block will be
/// sentover a ringbuffer.  In future, if pitch bending is added, it is probable that this will be done generically and
/// on the audio thread.  In that world, the case of non-looping sources can be handled by pumping it with zeros, since
/// pitch bending zeros is still zeros.
pub(crate) struct Resampler {
    reader: BufferedSourceReader,

    /// Rubato is not `Sync` (why?) so we must use a mutex to wrap it.
    ///
    /// This is a spinlock which is in practice never contended and so becomes some useless atomic ops; the performance
    /// overhead should thus be lost in the noise.
    resampler: spin::Mutex<SincFixedOut<f32>>,

    /// Data is streamed into and out of this vec before uninterleaving.
    interleaved_buffer: Vec<f32>,

    /// Uninterleaved data is streamed into this buffer, which is always filled completely from Rubato, then drained
    /// completely.
    uninterleaved_buffer: SplittableBuffer<Vec<f32>>,
}

impl Resampler {
    /// Given a wrapped source, create a resampler which will resample to Synthizer's hardcoded sample rate.
    pub(crate) fn new(reader: BufferedSourceReader) -> Result<Self, SampleSourceError> {
        let src_sr = reader.descriptor().sample_rate.get();
        // We are resampling with the ratio out/in, e.g. 48khz to 44.1khz is 41/48.  Docs in Rubato are unclear that
        // this is the way it goes.
        let ratio = (SR as f64) / (src_sr as f64);
        let params = SincInterpolationParameters {
            // probably excessive, but let's start by following Rubato's example.
            sinc_len: 256,
            // Really unclear why this is exposed, because the math says that it varies based on the ratio.
            f_cutoff: 0.95,
            // [JOS](https://ccrma.stanford.edu/~jos/resample/resample.html) says that linear is always fine, so again
            // unclear why Rubato is giving us these options.
            interpolation: rubato::SincInterpolationType::Linear,

            // Docs say start at 128.
            oversampling_factor: 128,

            // In theory, window doesn't matter much.  Again unclear why Rubato didn't just pick one for us, as JOS also
            // makes it kind of clear that this is somewhat arbitrary.
            window: rubato::WindowFunction::Blackman,
        };

        let resampler = SincFixedOut::<f32>::new(
            ratio,
            1.0,
            params,
            BLOCK_SIZE,
            reader.descriptor().get_channel_count(),
        )
        .map_err(|x| SampleSourceError::new_boxed(Box::new(x)))?;

        // Now set up our buffers.
        let max_input_frames = resampler.input_frames_max();

        let max_output_frames = resampler.output_frames_max();
        // I don't trust Rubato yet.  This assert doesn't hurt anything.
        assert_eq!(max_output_frames, BLOCK_SIZE);

        let chans = reader.descriptor().get_channel_count();
        let input_size_samples = max_input_frames * chans;

        let interleaved_buffer = vec![0.0f32; input_size_samples];

        let uninterleaved_buffer = {
            let backing = vec![0.0f32; input_size_samples];
            SplittableBuffer::<Vec<f32>>::new(
                backing,
                reader.descriptor().channel_format.get_channel_count(),
            )
        };

        Ok(Self {
            reader,
            interleaved_buffer,
            resampler: spin::Mutex::new(resampler),
            uninterleaved_buffer,
        })
    }

    /// Tick this resampler, producing the needed data.
    ///
    /// The destination slice must be `chans * BLOCK_SIZE` elements.
    pub(crate) fn read_samples(
        &mut self,
        destination: &mut [f32],
    ) -> Result<(), SampleSourceError> {
        let chans = self.reader.descriptor().get_channel_count();

        assert_eq!(
            destination.len(),
            self.reader.descriptor().get_channel_count() * BLOCK_SIZE
        );

        let mut resampler = self.resampler.lock();

        // Ask the underlying reader for samples, then zero anything remaining.
        let needed_frames = resampler.input_frames_next();
        assert_eq!(resampler.output_frames_next(), BLOCK_SIZE);
        let needed_samples = needed_frames * self.reader.descriptor().get_channel_count();
        assert!(
            needed_samples <= self.interleaved_buffer.len(),
            "{needed_samples} {}",
            self.interleaved_buffer.len()
        );

        // First fill the uninterleaved buffer
        let got = self
            .reader
            .read_samples(&mut self.interleaved_buffer[..needed_samples])?;
        if got < needed_samples as u64 {
            self.interleaved_buffer[(got as usize * chans)..].fill(0.0f32);
        }

        // Uninterleave, preparing for rubato.
        {
            let mut split = self.uninterleaved_buffer.split_mut();
            for c in 0..chans {
                let dst = &mut split[c];
                for i in 0..needed_frames {
                    let src_ind = i * chans + c;
                    dst[i] = self.interleaved_buffer[src_ind];
                }
            }
        }

        // Finally, actually do it.
        {
            let mut splittable_out = SplittableBuffer::new(
                destination,
                self.reader.descriptor().channel_format.get_channel_count(),
            );
            let mut split_out = splittable_out.split_mut();
            let split_uninterleaved = self
                .uninterleaved_buffer
                .split()
                .into_iter()
                .map(|x| &x[..needed_frames])
                .collect::<arrayvec::ArrayVec<&[f32], MAX_CHANNELS>>();

            resampler
                .process_into_buffer(&split_uninterleaved[..], &mut split_out[..], None).expect("If we are using Rubato correctly, it should always have exactly as much as it asks for");
        }

        Ok(())
    }

    pub(crate) fn descriptor(&self) -> &crate::sample_sources::Descriptor {
        self.reader.descriptor()
    }

    pub(crate) fn config_looping(&mut self, spec: crate::LoopSpec) {
        self.reader.config_looping(spec);
    }

    /// Seek the source to a new position where `new_pos` is in the sampling rate of the source.
    pub(crate) fn seek(&mut self, new_pos: u64) -> Result<(), SampleSourceError> {
        self.reader.seek(new_pos)
    }
}

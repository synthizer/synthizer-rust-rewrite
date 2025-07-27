//! Conditional resampling module that uses Rubato when needed or passes through data when not.

use rubato::{Resampler as RubatoResampler, SincFixedIn, SincInterpolationParameters};
use smallvec::SmallVec;

use crate::config::MAX_CHANNELS;

#[derive(Debug, thiserror::Error)]
pub enum ResamplingError {
    #[error("Failed to create Rubato resampler: {0}")]
    RubatoError(#[from] rubato::ResamplerConstructionError),
    #[error("Failed to process samples: {0}")]
    ProcessError(#[from] rubato::ResampleError),
    #[error("Invalid channel count: {0}")]
    InvalidChannelCount(usize),
}

/// A conditional resampler that only creates a Rubato instance when resampling is actually needed.
pub struct ConditionalResampler {
    /// The source sample rate
    source_rate: u32,
    /// The target sample rate
    target_rate: u32,
    /// Number of channels
    channels: usize,
    /// The actual resampler, only created when needed
    resampler: Option<ResamplerState>,
    /// Number of frames.
    input_frames: usize,
}

struct ResamplerState {
    resampler: SincFixedIn<f32>,
    /// Buffer for uninterleaved data (required by Rubato)
    uninterleaved_buffer: Vec<Vec<f32>>,
    /// Buffer for uninterleaved output
    output_buffer: Vec<Vec<f32>>,
}

impl ConditionalResampler {
    /// Create a new conditional resampler.
    pub fn new(
        source_rate: u32,
        target_rate: u32,
        channels: usize,
        input_frames: usize,
    ) -> Result<Self, ResamplingError> {
        if channels == 0 || channels > 16 {
            return Err(ResamplingError::InvalidChannelCount(channels));
        }

        let resampler = if source_rate != target_rate {
            Some(ResamplerState::new(
                source_rate,
                target_rate,
                channels,
                input_frames,
            )?)
        } else {
            None
        };

        Ok(Self {
            source_rate,
            target_rate,
            channels,
            resampler,
            input_frames,
        })
    }

    /// Process audio data, resampling if needed.
    ///
    /// The input slice should contain interleaved samples.
    /// The output slice should have capacity for the expected output.
    ///
    /// Returns the number of output frames written.
    pub fn process<T, U>(&mut self, input: T, output: &mut U) -> Result<usize, ResamplingError>
    where
        T: AsRef<[f32]>,
        U: AsMut<[f32]> + ?Sized,
    {
        let input = input.as_ref();
        let output = output.as_mut();

        if let Some(state) = &mut self.resampler {
            // Resampling is required
            state.process(input, output, self.channels)
        } else {
            // No resampling needed, just copy
            let samples_to_copy = input.len();
            let frames_to_copy = samples_to_copy / self.channels;

            output[..samples_to_copy].copy_from_slice(&input[..samples_to_copy]);
            Ok(frames_to_copy)
        }
    }

    /// Get the maximum number of input frames the resampler might need.
    pub fn input_frames_max(&self) -> usize {
        self.input_frames
    }

    /// Get the maximum number of output frames the resampler might produce.
    pub fn output_frames_max(&self) -> usize {
        if let Some(state) = &self.resampler {
            state.resampler.output_frames_max()
        } else {
            // When not resampling, output equals input
            self.input_frames
        }
    }

    /// Get the number of input frames needed for the next call to process.
    pub fn input_frames_next(&self) -> usize {
        self.input_frames
    }

    /// Get the number of output frames that will be produced by the next call to process.
    pub fn output_frames_next(&self) -> usize {
        if let Some(state) = &self.resampler {
            state.resampler.output_frames_next()
        } else {
            // When not resampling, output equals input
            self.input_frames
        }
    }

    /// Check if resampling is active.
    pub fn is_resampling(&self) -> bool {
        self.resampler.is_some()
    }
}

impl ResamplerState {
    fn new(
        source_rate: u32,
        target_rate: u32,
        channels: usize,
        input_frames: usize,
    ) -> Result<Self, ResamplingError> {
        let ratio = (target_rate as f64) / (source_rate as f64);

        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            interpolation: rubato::SincInterpolationType::Linear,
            oversampling_factor: 128,
            window: rubato::WindowFunction::Blackman,
        };

        let resampler = SincFixedIn::<f32>::new(ratio, 1.0, params, input_frames, channels)?;

        // Output can vary
        let max_output_frames = resampler.output_frames_max();
        let uninterleaved_buffer = (0..channels).map(|_| vec![0.0f32; input_frames]).collect();
        let output_buffer = (0..channels)
            .map(|_| vec![0.0f32; max_output_frames])
            .collect();

        Ok(Self {
            resampler,
            uninterleaved_buffer,
            output_buffer,
        })
    }

    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        channels: usize,
    ) -> Result<usize, ResamplingError> {
        let input_frames = self.resampler.input_frames_next();
        let output_frames = self.resampler.output_frames_next();

        // Uninterleave input
        for ch in 0..channels {
            for frame in 0..input_frames {
                self.uninterleaved_buffer[ch][frame] = input[frame * channels + ch];
            }
        }

        let did;

        // Process through Rubato
        {
            let input_refs: SmallVec<[&[f32]; MAX_CHANNELS]> = self
                .uninterleaved_buffer
                .iter()
                .take(channels)
                .map(|v| &v[..input_frames])
                .collect();
            let mut output_refs: SmallVec<[&mut [f32]; MAX_CHANNELS]> = self
                .output_buffer
                .iter_mut()
                .take(channels)
                .map(|v| &mut v[..output_frames])
                .collect();

            // Contrary to Rubato's API docs, sometimes this does a different number of frames than was claimed.
            // Might be fixed in newer versions.
            did = self
                .resampler
                .process_into_buffer(&input_refs, &mut output_refs, None)?
                .1;
        }

        // Interleave output
        for ch in 0..channels {
            for frame in 0..did {
                let out_idx = frame * channels + ch;

                output[out_idx] = self.output_buffer[ch][frame];
            }
        }

        Ok(did)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_resampling_passthrough() {
        let mut resampler = ConditionalResampler::new(44100, 44100, 2, 3).unwrap();
        assert!(!resampler.is_resampling());

        let input = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]; // 3 stereo frames
        let mut output = vec![0.0; 6];

        let frames = resampler.process(&input, &mut output).unwrap();
        assert_eq!(frames, 3);
        assert_eq!(output, input);
    }

    #[test]
    fn test_resampling_active() {
        let resampler = ConditionalResampler::new(48000, 44100, 2, 10).unwrap();
        assert!(resampler.is_resampling());
    }
}

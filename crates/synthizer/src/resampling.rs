//! Conditional resampling module that uses Rubato when needed or passes through data when not.

use rubato::{
    Resampler as RubatoResampler, SincFixedIn, SincFixedOut, SincInterpolationParameters,
};
use sync_wrapper::SyncWrapper;

/// Object-safe trait for resamplers that work with interleaved audio data.
trait ResamplerTrait: Send + Sync {
    /// Process audio data, resampling if needed.
    /// Returns the number of input and output frames used.
    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<(usize, usize), ResamplingError>;

    /// Get the maximum number of input frames the resampler might need.
    fn input_frames_max(&mut self) -> usize;

    /// Get the maximum number of output frames the resampler might produce.  
    fn output_frames_max(&mut self) -> usize;

    /// Get the number of input frames needed for the next call to process.
    fn input_frames_next(&mut self) -> usize;

    /// Get the number of output frames that will be produced by the next call to process.
    fn output_frames_next(&mut self) -> usize;

    /// Check if resampling is active.
    fn is_resampling(&self) -> bool;
}

#[derive(Debug, Clone)]
pub enum ResamplerMode {
    /// Fixed input size, variable output (for decoding)
    FixedInput { input_frames: usize },
    /// Fixed output size, variable input (for block-based processing)
    FixedOutput { output_frames: usize },
}

#[derive(Debug, thiserror::Error)]
pub enum ResamplingError {
    #[error("Failed to create Rubato resampler: {0}")]
    RubatoError(#[from] rubato::ResamplerConstructionError),
    #[error("Failed to process samples: {0}")]
    ProcessError(#[from] rubato::ResampleError),
    #[error("Invalid channel count: {0}")]
    InvalidChannelCount(usize),
}

fn deinterleave(input: &[f32], output: &mut [Vec<f32>], frames: usize) {
    let channels = output.len();
    debug_assert!(input.len() >= frames * channels);
    debug_assert_eq!(input.len() % channels, 0);
    debug_assert!(channels > 0);
    debug_assert!(output[0].len() >= frames);

    for frame in 0..frames {
        for channel in 0..channels {
            output[channel][frame] = input[frame * channels + channel];
        }
    }
}

fn interleave(input: &[Vec<f32>], output: &mut [f32], frames: usize) {
    let channels = input.len();
    debug_assert!(channels > 0);
    debug_assert!(output.len() >= frames * channels);

    for frame in 0..frames {
        for channel in 0..channels {
            output[frame * channels + channel] = input[channel][frame];
        }
    }
}

/// Wrapper around Rubato's FixedIn resampler that implements our ResamplerTrait
struct FixedInResampler {
    resampler: SyncWrapper<Box<SincFixedIn<f32>>>,
    uninterleaved_buffer: Vec<Vec<f32>>,
    output_buffer: Vec<Vec<f32>>,
    channels: usize,
}

impl FixedInResampler {
    fn new(
        source_rate: u32,
        target_rate: u32,
        channels: usize,
        input_frames: usize,
    ) -> Result<Self, ResamplingError> {
        let params = SincInterpolationParameters {
            sinc_len: 64,
            f_cutoff: 0.95,
            interpolation: rubato::SincInterpolationType::Cubic,
            oversampling_factor: 128,
            window: rubato::WindowFunction::Blackman2,
        };

        let resampler = SincFixedIn::<f32>::new(
            target_rate as f64 / source_rate as f64,
            1.0,
            params,
            input_frames,
            channels,
        )?;

        let uninterleaved_buffer = vec![vec![0.0; input_frames]; channels];
        let max_output_frames = resampler.output_frames_max();
        let output_buffer = vec![vec![0.0; max_output_frames]; channels];

        Ok(Self {
            resampler: SyncWrapper::new(Box::new(resampler)),
            uninterleaved_buffer,
            output_buffer,
            channels,
        })
    }
}

impl ResamplerTrait for FixedInResampler {
    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<(usize, usize), ResamplingError> {
        let input_frames = input.len() / self.channels;
        deinterleave(input, &mut self.uninterleaved_buffer, input_frames);

        let ret = self.resampler.get_mut().process_into_buffer(
            &self.uninterleaved_buffer,
            &mut self.output_buffer,
            None,
        )?;

        interleave(&self.output_buffer, output, ret.1);

        Ok(ret)
    }

    fn input_frames_max(&mut self) -> usize {
        self.resampler.get_mut().input_frames_max()
    }

    fn output_frames_max(&mut self) -> usize {
        self.resampler.get_mut().output_frames_max()
    }

    fn input_frames_next(&mut self) -> usize {
        self.resampler.get_mut().input_frames_next()
    }

    fn output_frames_next(&mut self) -> usize {
        self.resampler.get_mut().output_frames_next()
    }

    fn is_resampling(&self) -> bool {
        true
    }
}

/// Wrapper around Rubato's FixedOut resampler that implements our ResamplerTrait  
struct FixedOutResampler {
    resampler: SyncWrapper<Box<SincFixedOut<f32>>>,
    uninterleaved_buffer: Vec<Vec<f32>>,
    output_buffer: Vec<Vec<f32>>,
    channels: usize,
}

impl FixedOutResampler {
    fn new(
        source_rate: u32,
        target_rate: u32,
        channels: usize,
        output_frames: usize,
    ) -> Result<Self, ResamplingError> {
        let params = SincInterpolationParameters {
            sinc_len: 64,
            f_cutoff: 0.95,
            interpolation: rubato::SincInterpolationType::Cubic,
            oversampling_factor: 128,
            window: rubato::WindowFunction::Blackman2,
        };

        let resampler = SincFixedOut::<f32>::new(
            target_rate as f64 / source_rate as f64,
            1.0,
            params,
            output_frames,
            channels,
        )?;

        let max_input_frames = resampler.input_frames_max();
        let uninterleaved_buffer = vec![vec![0.0; max_input_frames]; channels];
        let output_buffer = vec![vec![0.0; output_frames]; channels];

        Ok(Self {
            resampler: SyncWrapper::new(Box::new(resampler)),
            uninterleaved_buffer,
            output_buffer,
            channels,
        })
    }
}

impl ResamplerTrait for FixedOutResampler {
    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<(usize, usize), ResamplingError> {
        let input_frames = self.input_frames_next().min(input.len() / self.channels);
        deinterleave(input, &mut self.uninterleaved_buffer, input_frames);

        let ret = self.resampler.get_mut().process_into_buffer(
            &self.uninterleaved_buffer,
            &mut self.output_buffer,
            None,
        )?;

        interleave(&self.output_buffer, output, ret.1);
        Ok(ret)
    }

    fn input_frames_max(&mut self) -> usize {
        self.resampler.get_mut().input_frames_max()
    }

    fn output_frames_max(&mut self) -> usize {
        self.resampler.get_mut().output_frames_max()
    }

    fn input_frames_next(&mut self) -> usize {
        self.resampler.get_mut().input_frames_next()
    }

    fn output_frames_next(&mut self) -> usize {
        self.resampler.get_mut().output_frames_next()
    }

    fn is_resampling(&self) -> bool {
        true
    }
}

/// A passthrough "resampler" for when no resampling is needed
struct PassthroughResampler {
    channels: usize,
    frames_per_process: usize,
}

impl PassthroughResampler {
    fn new(channels: usize, frames_per_process: usize) -> Self {
        Self {
            channels,
            frames_per_process,
        }
    }
}

impl ResamplerTrait for PassthroughResampler {
    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<(usize, usize), ResamplingError> {
        let input_frames = input.len() / self.channels;
        let output_frames = output.len() / self.channels;
        let frames = input_frames.min(output_frames);

        let samples = frames * self.channels;
        output[..samples].copy_from_slice(&input[..samples]);
        Ok((frames, frames))
    }

    fn input_frames_max(&mut self) -> usize {
        self.frames_per_process
    }

    fn output_frames_max(&mut self) -> usize {
        self.frames_per_process
    }

    fn input_frames_next(&mut self) -> usize {
        self.frames_per_process
    }

    fn output_frames_next(&mut self) -> usize {
        self.frames_per_process
    }

    fn is_resampling(&self) -> bool {
        false
    }
}

/// A conditional resampler that only creates a Rubato instance when resampling is actually needed.
pub struct ConditionalResampler {
    /// The actual resampler implementation
    resampler: Box<dyn ResamplerTrait>,
}

impl ConditionalResampler {
    /// Create a new conditional resampler.
    pub fn new(
        source_rate: u32,
        target_rate: u32,
        channels: usize,
        mode: ResamplerMode,
    ) -> Result<Self, ResamplingError> {
        if channels == 0 || channels > 16 {
            return Err(ResamplingError::InvalidChannelCount(channels));
        }

        let resampler: Box<dyn ResamplerTrait> =
            if source_rate != target_rate {
                // Need actual resampling
                match mode {
                    ResamplerMode::FixedInput { input_frames } => Box::new(FixedInResampler::new(
                        source_rate,
                        target_rate,
                        channels,
                        input_frames,
                    )?),
                    ResamplerMode::FixedOutput { output_frames } => Box::new(
                        FixedOutResampler::new(source_rate, target_rate, channels, output_frames)?,
                    ),
                }
            } else {
                // No resampling needed - use passthrough
                let frames = match mode {
                    ResamplerMode::FixedInput { input_frames } => input_frames,
                    ResamplerMode::FixedOutput { output_frames } => output_frames,
                };
                Box::new(PassthroughResampler::new(channels, frames))
            };

        Ok(Self { resampler })
    }

    /// Process audio data, resampling if needed.
    ///
    /// The input slice should contain interleaved samples.
    /// The output slice should have capacity for the expected output.
    ///
    /// Returns the number of output frames written.
    pub fn process<T, U>(
        &mut self,
        input: T,
        output: &mut U,
    ) -> Result<(usize, usize), ResamplingError>
    where
        T: AsRef<[f32]>,
        U: AsMut<[f32]> + ?Sized,
    {
        let input = input.as_ref();
        let output = output.as_mut();
        self.resampler.process(input, output)
    }

    /// Get the maximum number of input frames the resampler might need.
    pub fn input_frames_max(&mut self) -> usize {
        self.resampler.input_frames_max()
    }

    /// Get the maximum number of output frames the resampler might produce.
    pub fn output_frames_max(&mut self) -> usize {
        self.resampler.output_frames_max()
    }

    /// Get the number of input frames needed for the next call to process.
    pub fn input_frames_next(&mut self) -> usize {
        self.resampler.input_frames_next()
    }

    /// Get the number of output frames that will be produced by the next call to process.
    pub fn output_frames_next(&mut self) -> usize {
        self.resampler.output_frames_next()
    }

    /// Check if resampling is active.
    pub fn is_resampling(&self) -> bool {
        self.resampler.is_resampling()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_resampling_passthrough() {
        let mut resampler = ConditionalResampler::new(
            44100,
            44100,
            2,
            ResamplerMode::FixedInput { input_frames: 3 },
        )
        .unwrap();
        assert!(!resampler.is_resampling());

        let input = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]; // 3 stereo frames
        let mut output = vec![0.0; 6];

        let frames = resampler.process(&input, &mut output).unwrap();
        assert_eq!(frames.1, 3);
        assert_eq!(output, input);
    }

    #[test]
    fn test_resampling_active() {
        let resampler = ConditionalResampler::new(
            48000,
            44100,
            2,
            ResamplerMode::FixedInput { input_frames: 10 },
        )
        .unwrap();
        assert!(resampler.is_resampling());
    }
}

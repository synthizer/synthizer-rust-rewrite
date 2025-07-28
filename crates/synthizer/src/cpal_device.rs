//! Simple cpal-based audio device output.
//!
//! This replaces the miniaudio dependency with pure Rust audio I/O.

use crate::data_structures::RefillableWrapper;
use crate::resampling::{ConditionalResampler, ResamplerMode};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleRate, Stream, StreamConfig};
use std::sync::Arc;

pub struct AudioDevice {
    stream: Stream,
    _config: Arc<DeviceConfig>,
}

#[derive(Debug, Clone)]
pub struct DeviceConfig {
    pub sample_rate: u32,
    pub channels: usize,
}

#[derive(Debug, Default, Clone)]
pub struct DeviceOptions {
    pub sample_rate: Option<u32>,
    pub channels: Option<usize>,
}

pub type DeviceCallback = Box<dyn FnMut(&mut [f32]) + Send>;

#[derive(Debug, thiserror::Error)]
pub enum AudioDeviceError {
    #[error("No audio device available")]
    NoDevice,
    #[error("Stream configuration not supported")]
    ConfigNotSupported,
    #[error("Failed to query device configs: {0}")]
    QueryConfigsFailed(#[from] cpal::SupportedStreamConfigsError),
    #[error("Failed to get default config: {0}")]
    DefaultConfigFailed(#[from] cpal::DefaultStreamConfigError),
    #[error("Failed to build stream: {0}")]
    BuildStreamFailed(#[from] cpal::BuildStreamError),
    #[error("Failed to start stream: {0}")]
    PlayStreamFailed(#[from] cpal::PlayStreamError),
}

/// Find the best output configuration for the given device and options.
fn find_best_config(
    device: &cpal::Device,
    options: &DeviceOptions,
) -> Result<cpal::SupportedStreamConfig, AudioDeviceError> {
    if let (Some(sr), Some(ch)) = (options.sample_rate, options.channels) {
        let supported_configs: Vec<_> = device.supported_output_configs()?.collect();

        if supported_configs.is_empty() {
            return Err(AudioDeviceError::ConfigNotSupported);
        }

        // Try to find the best match using a scoring system
        let mut best_config = None;
        let mut best_score = i32::MIN;

        for supported in &supported_configs {
            let channels = supported.channels() as usize;
            let can_do_sample_rate =
                supported.min_sample_rate().0 <= sr && supported.max_sample_rate().0 >= sr;

            // Skip configs with less than 2 channels
            if channels < 2 {
                continue;
            }

            // Score based on:
            // - Exact channel match: +1000
            // - More channels than requested (we can downmix): +500
            // - Exact sample rate: +100
            // - Higher sample rate (we can downsample): +50
            let mut score = 0;

            if channels == ch {
                score += 1000;
            } else if channels >= ch {
                score += 500;
            }

            if can_do_sample_rate {
                score += 100;
            } else if supported.max_sample_rate().0 >= sr {
                score += 50;
            }

            if score > best_score {
                best_score = score;
                let sample_rate = if can_do_sample_rate {
                    SampleRate(sr)
                } else {
                    // Use the highest available sample rate
                    supported.max_sample_rate()
                };
                best_config = Some(supported.with_sample_rate(sample_rate));
            }
        }

        best_config.ok_or(AudioDeviceError::ConfigNotSupported)
    } else {
        device.default_output_config().map_err(Into::into)
    }
}

impl AudioDevice {
    /// Open the default audio output device with the given options.
    ///
    /// The callback will be called with fixed-size blocks according to BLOCK_SIZE, regardless of the actual device's
    /// frame size. Resampling will be applied automatically if the device sample rate differs from Synthizer's internal
    /// rate. Channel conversion from Synthizer's stereo output to the device's channel count is handled automatically.
    pub fn open_default(
        options: DeviceOptions,
        mut callback: impl FnMut(&mut [f32]) + Send + 'static,
    ) -> Result<Self, AudioDeviceError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioDeviceError::NoDevice)?;

        // Get the best config for the device
        let config = find_best_config(&device, &options)?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;

        // Ensure we have at least stereo
        if channels < 2 {
            return Err(AudioDeviceError::ConfigNotSupported);
        }

        let device_config = Arc::new(DeviceConfig {
            sample_rate,
            channels,
        });

        let stream_config: StreamConfig = config.into();

        let err_fn = |err| {
            panic!("Device error! {err:?}");
        };

        // Set up resampling - ConditionalResampler handles the no-resampling case internally
        let synthizer_rate = crate::config::SR as u32;
        let mut resampler = ConditionalResampler::new(
            synthizer_rate,
            sample_rate,
            2,
            ResamplerMode::FixedInput {
                input_frames: crate::config::BLOCK_SIZE,
            },
        )
        .map_err(|e| {
            AudioDeviceError::BuildStreamFailed(cpal::BuildStreamError::BackendSpecific {
                err: cpal::BackendSpecificError {
                    description: e.to_string(),
                },
            })
        })?;

        // Buffer for Synthizer's fixed-size output blocks
        let mut synthizer_buffer = vec![0.0f32; crate::config::BLOCK_SIZE * 2];

        // This buffer always holds stereo data from the resampler
        let resampled_buffer = vec![0.0f32; resampler.output_frames_max() * 2];
        let mut resampled_buffer = RefillableWrapper::new(resampled_buffer);

        let stream = device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let wanted_frames = data.len() / channels;
                let mut done_frames = 0usize;

                while done_frames < wanted_frames {
                    // Check how many stereo frames are available in the resampled buffer
                    let available_frames = resampled_buffer.available() / 2;
                    let frames_to_copy = (wanted_frames - done_frames).min(available_frames);

                    if frames_to_copy == 0 {
                        // Buffer is empty, need to generate more audio
                        let will_produce_frames = resampler.output_frames_next();
                        let will_produce_samples = will_produce_frames * 2; // stereo

                        let dest = resampled_buffer
                            .refill_start(will_produce_samples)
                            .expect("RefillableWrapper should always have space after consuming");

                        // Generate one block of audio from Synthizer
                        callback(&mut synthizer_buffer);

                        // Resample from Synthizer's rate to device rate (both stereo)
                        let (_, frames_written) = resampler
                            .process(&synthizer_buffer, dest)
                            .expect("Resampling failed");

                        resampled_buffer.refill_end(frames_written * 2);
                        continue;
                    }

                    // Copy available frames from resampled buffer to output
                    let consuming = resampled_buffer.consume_start();
                    let consume_samples = frames_to_copy * 2; // stereo samples

                    // Convert from stereo to device channel count
                    let output_offset = done_frames * channels;
                    match channels {
                        2 => {
                            // Direct copy for stereo
                            data[output_offset..output_offset + consume_samples]
                                .copy_from_slice(&consuming[..consume_samples]);
                        }
                        _ => {
                            // Convert stereo to N channels
                            for frame in 0..frames_to_copy {
                                let left = consuming[frame * 2];
                                let right = consuming[frame * 2 + 1];
                                let out_base = output_offset + frame * channels;

                                for ch in 0..channels {
                                    data[out_base + ch] = match ch {
                                        0 => left,
                                        1 => right,
                                        _ => 0.0, // Fill extra channels with silence
                                    };
                                }
                            }
                        }
                    }

                    resampled_buffer.consume_end(consume_samples);
                    done_frames += frames_to_copy;
                }
            },
            err_fn,
            None,
        )?;

        Ok(AudioDevice {
            stream,
            _config: device_config,
        })
    }

    /// Start audio playback.
    pub fn start(&self) -> Result<(), AudioDeviceError> {
        self.stream.play().map_err(Into::into)
    }

    /// Pause audio playback.
    pub fn pause(&self) -> Result<(), cpal::PauseStreamError> {
        self.stream.pause()
    }
}

/// Get the sample rate and channel count from the default output device.
pub fn get_default_device_config() -> Result<DeviceConfig, cpal::DefaultStreamConfigError> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or(cpal::DefaultStreamConfigError::DeviceNotAvailable)?;

    let config = device.default_output_config()?;

    Ok(DeviceConfig {
        sample_rate: config.sample_rate().0,
        channels: config.channels() as usize,
    })
}

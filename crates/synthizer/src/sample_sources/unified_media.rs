//! Unified media source implementation that combines decoding, looping, and resampling.

use std::io::{Read, Seek};

use crate::config::*;
use crate::error::Result;
use crate::loop_spec::LoopSpec;
use crate::resampling::{ConditionalResampler, ResamplerMode};
use crate::sample_sources::Descriptor;

use super::execution::loop_driver::{LoopDriver, ReadOp};
use super::symphonia_impl;

const FRAMES_IN_RING: usize =
    (crate::config::SR as usize / 10).next_multiple_of(crate::config::BLOCK_SIZE);

/// A unified media source that handles all media decoding needs.
pub struct UnifiedMediaSource {
    /// The Symphonia decoder wrapper
    symphonia: symphonia_impl::SymphoniaWrapper,

    /// Loop driver for handling looping logic
    loop_driver: LoopDriver,

    resampler: ConditionalResampler,
    resampler_input_buffer: Vec<f32>,
    resampler_output_buffer: Vec<f32>,

    /// Our target sample rate
    target_sample_rate: u32,
}

struct SymphoniaStream<S>(S);

impl<S> std::io::Read for SymphoniaStream<S>
where
    S: std::io::Read,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl<S: std::io::Seek> std::io::Seek for SymphoniaStream<S> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}

impl<S: std::io::Read + std::io::Seek + Send + Sync + 'static> symphonia::core::io::MediaSource
    for SymphoniaStream<S>
{
    fn byte_len(&self) -> Option<u64> {
        None
    }

    fn is_seekable(&self) -> bool {
        true
    }
}

impl UnifiedMediaSource {
    /// Create a new unified media source from any Read + Seek source.
    pub fn new<S>(source: S, target_sample_rate: u32) -> Result<Self, crate::error::Error>
    where
        S: Read + Seek + Send + Sync + 'static,
    {
        let symphonia = symphonia_impl::build_symphonia(SymphoniaStream(source))?;

        // Get initial info from symphonia
        let source_descriptor = symphonia.get_descriptor();
        let source_sample_rate = source_descriptor.sample_rate.get() as u32;

        // Create loop driver
        let loop_driver = LoopDriver::new(source_sample_rate as u64);

        // Set up resampler if needed

        let channels = source_descriptor.channel_format.get_channel_count().get();
        let mut resampler = ConditionalResampler::new(
            source_sample_rate,
            target_sample_rate,
            channels,
            ResamplerMode::FixedOutput {
                output_frames: BLOCK_SIZE,
            },
        )?;

        let max_input_frames = resampler.input_frames_max();
        let resampler_input_buffer = vec![0.0f32; max_input_frames * channels];
        let resampler_output_buffer = vec![0.0f32; resampler.output_frames_max() * channels];

        Ok(Self {
            symphonia,
            loop_driver,
            resampler,
            resampler_input_buffer,
            resampler_output_buffer,
            target_sample_rate,
        })
    }

    pub(crate) fn into_task_and_handle(self) -> Result<(CrossThreadTask, MediaController)> {
        let (command_tx, command_rx) = crossbeam::channel::unbounded();
        let (writer, reader) = audio_synchronization::spsc_ring::create_ring(
            FRAMES_IN_RING * self.get_channel_count(),
        );

        let handle = MediaController {
            command_tx,
            ring: Some(reader),
            descriptor: self.get_descriptor().clone(),
        };

        let task = CrossThreadTask {
            media: self,
            command_rx,
            out_ring: writer,
            playing: false,
        };

        Ok((task, handle))
    }

    /// Configure looping for this source.
    pub fn set_looping(&mut self, loop_spec: LoopSpec) -> Result<(), crate::error::Error> {
        self.loop_driver.config_looping(loop_spec)
    }

    fn get_channel_count(&self) -> usize {
        self.symphonia
            .get_descriptor()
            .channel_format
            .get_channel_count()
            .get()
    }

    /// Read and resample audio data.
    fn read_and_resample(&mut self, output: &mut [f32]) -> Result<u64> {
        let channels = self.get_channel_count();

        let input_frames_needed = self.resampler.input_frames_next();
        let input_samples_needed = input_frames_needed * channels;

        // Read from symphonia into our input buffer
        let frames_read = self
            .symphonia
            .read_samples(&mut self.resampler_input_buffer[..input_samples_needed])?;

        if frames_read == 0 {
            return Ok(0);
        }

        // Resample
        let (_, output_frames) = self.resampler.process(
            &self.resampler_input_buffer[..frames_read as usize * channels],
            output,
        )?;

        Ok(output_frames as u64)
    }

    pub fn get_descriptor(&self) -> &Descriptor {
        self.symphonia.get_descriptor()
    }

    pub fn read_samples(&mut self, destination: &mut [f32]) -> Result<u64> {
        let channels = self.get_channel_count();
        let dest_frames = destination.len() / channels;
        let mut frames_written = 0u64;

        while frames_written < dest_frames as u64 {
            let remaining_frames = dest_frames as u64 - frames_written;

            // Ask loop driver what to do
            match self.loop_driver.pre_read(remaining_frames) {
                ReadOp::Read(frames_to_read) => {
                    let dest_start = frames_written as usize * channels;
                    let dest_slice = &mut destination
                        [dest_start..dest_start + (frames_to_read as usize * channels)];

                    let frames_read = self.read_and_resample(dest_slice)?;

                    if frames_read == 0 {
                        self.loop_driver.observe_eof();
                    } else {
                        self.loop_driver.observe_read(frames_read);
                        frames_written += frames_read;
                    }
                }
                ReadOp::Seek(position) => {
                    self.symphonia.seek(position)?;
                    self.loop_driver.observe_seek(position);
                }
                ReadOp::ReachedEof => {
                    break;
                }
            }
        }

        Ok(frames_written)
    }

    pub fn seek(&mut self, position_in_frames: u64) -> Result<()> {
        self.symphonia.seek(position_in_frames)?;
        self.loop_driver.observe_seek(position_in_frames);
        Ok(())
    }

    pub fn is_finished(&self) -> bool {
        self.loop_driver.is_finished()
    }
}

enum CrossThreadCommand {
    Seek(u64),
    Pause,
    Play,
    SetLooping(LoopSpec),
}

pub(crate) struct CrossThreadTask {
    media: UnifiedMediaSource,
    command_rx: crossbeam::channel::Receiver<CrossThreadCommand>,
    out_ring: audio_synchronization::spsc_ring::RingWriter<f32>,
    playing: bool,
}

impl CrossThreadTask {
    fn execute_fallible(&mut self) -> Result<bool> {
        loop {
            let cmd = match self.command_rx.try_recv() {
                Ok(cmd) => cmd,
                Err(e) if e.is_disconnected() => return Ok(false),
                Err(_) => break,
            };

            match cmd {
                CrossThreadCommand::Seek(position) => {
                    self.media.seek(position)?;
                }
                CrossThreadCommand::Pause => {
                    self.playing = false;
                }
                CrossThreadCommand::Play => {
                    self.playing = true;
                }
                CrossThreadCommand::SetLooping(loop_spec) => {
                    self.media.set_looping(loop_spec)?;
                }
            }
        }

        if self.playing {
            let mut res = Ok(0u64);

            self.out_ring.write_slices(|slices| {
                let Some(slices) = slices else {
                    return 0;
                };

                res = self.media.read_samples(slices.0).and_then(|first_done| {
                    let Some(second) = slices.1 else {
                        return Ok(first_done);
                    };

                    self.media
                        .read_samples(second)
                        .map(|second_done| first_done + second_done)
                });

                res.as_ref().map(|x| *x as usize).unwrap_or(0) * self.media.get_channel_count()
            });

            res?;
        }

        Ok(true)
    }
}

impl crate::worker_pool::Task for CrossThreadTask {
    fn execute(&mut self) -> bool {
        self.execute_fallible()
            .inspect_err(|e| {
                rt_error!("Error executing cross-thread media task: {e:?}");
            })
            .unwrap_or(false)
    }
}

pub struct MediaController {
    command_tx: crossbeam::channel::Sender<CrossThreadCommand>,

    // Stolen when making the media signal.
    pub(crate) ring: Option<audio_synchronization::spsc_ring::RingReader<f32>>,

    pub(crate) descriptor: Descriptor,
}

impl MediaController {
    pub fn play(&self) -> Result<()> {
        self.command_tx.send(CrossThreadCommand::Play)?;
        Ok(())
    }

    pub fn pause(&self) -> Result<()> {
        self.command_tx.send(CrossThreadCommand::Pause)?;

        Ok(())
    }

    pub fn seek(&self, position: u64) -> Result<()> {
        self.command_tx.send(CrossThreadCommand::Seek(position))?;

        Ok(())
    }

    pub fn set_looping(&self, loop_spec: LoopSpec) -> Result<()> {
        self.command_tx
            .send(CrossThreadCommand::SetLooping(loop_spec))?;

        Ok(())
    }

    pub fn get_sr(&self) -> u64 {
        self.descriptor.sample_rate.get()
    }

    pub fn get_channels(&self) -> usize {
        self.descriptor.channel_format.get_channel_count().get()
    }
}

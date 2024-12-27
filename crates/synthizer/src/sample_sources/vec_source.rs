use std::num::NonZeroU64;

use super::*;
use crate::channel_format::ChannelFormat;

#[derive(Debug, Default)]
pub struct VecSourceBuilder {
    channel_format: Option<ChannelFormat>,
    sample_rate: Option<NonZeroU64>,
}

/// A source of samples wrapping a vec.
pub struct VecSource {
    data: Vec<f32>,
    channel_format: ChannelFormat,
    sample_rate: NonZeroU64,
    position_in_frames: usize,
    frame_count: usize,
}

impl VecSource {
    pub fn builder() -> VecSourceBuilder {
        Default::default()
    }
}

impl VecSourceBuilder {
    /// Set the channel format of this source.
    ///
    /// This is a required setting.
    pub fn set_channel_format(mut self, channel_format: ChannelFormat) -> Self {
        self.channel_format = Some(channel_format);
        self
    }

    /// Set the sample rate of this source.
    ///
    /// This is a required setting.
    pub fn set_sample_rate(mut self, sample_rate: NonZeroU64) -> Self {
        self.sample_rate = Some(sample_rate);
        self
    }

    /// Build this source if possible.
    ///
    /// All fields must have been set or an error results.
    pub fn build_with_data(self, data: Vec<f32>) -> Result<VecSource, SampleSourceError> {
        validate_required_fields!(self, channel_format, sample_rate);

        if data.len() % channel_format.get_channel_count().get() != 0 {
            return Err(
                "The passed-in data has a length which is not a multiple of the channel count"
                    .into(),
            );
        }

        Ok(VecSource {
            frame_count: data.len() / channel_format.get_channel_count().get(),
            channel_format,
            data,
            sample_rate,
            position_in_frames: 0,
        })
    }
}

impl SampleSource for VecSource {
    fn get_descriptor(&self) -> Descriptor {
        Descriptor {
            channel_format: self.channel_format,
            duration: Some(self.frame_count as u64),
            sample_rate: self.sample_rate,
            seek_support: super::SeekSupport::SampleAccurate,
            latency: super::Latency::AudioThreadSafe,
        }
    }

    fn seek(&mut self, position_in_frames: u64) -> Result<(), SampleSourceError> {
        assert!(position_in_frames < self.frame_count as u64);
        self.position_in_frames = position_in_frames as usize;
        Ok(())
    }

    fn read_samples(&mut self, destination: &mut [f32]) -> Result<u64, SampleSourceError> {
        let chan_count = self.channel_format.get_channel_count().get();
        assert_eq!(destination.len() % chan_count, 0);
        let wanted_frames = destination.len() / chan_count;
        let available_frames = self.data.len() / chan_count - self.position_in_frames;
        let will_do = available_frames.min(wanted_frames);
        let pos_in_samples = chan_count * self.position_in_frames;
        let src_slice = &self.data[pos_in_samples..pos_in_samples + will_do * chan_count];
        destination[..will_do * chan_count].copy_from_slice(src_slice);
        self.position_in_frames += will_do;
        Ok(will_do as u64)
    }
}

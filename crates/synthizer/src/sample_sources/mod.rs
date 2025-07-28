pub(crate) mod execution;
mod symphonia_impl;
mod unified_media;

pub use unified_media::{MediaController, UnifiedMediaSource};

use std::num::NonZeroU64;

/// Describes the characteristics of a source.
#[derive(Debug, Clone)]
pub struct Descriptor {
    pub(crate) sample_rate: NonZeroU64,
    pub(crate) duration: u64,
    pub(crate) channel_format: crate::channel_format::ChannelFormat,
}

impl Descriptor {
    pub(crate) fn get_channel_count(&self) -> usize {
        self.channel_format.get_channel_count().get()
    }
}

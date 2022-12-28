use std::num::NonZeroUsize;

/// A format for audio data.
#[derive(Clone, Debug, derive_more::IsVariant)]
pub enum ChannelFormat {
    /// This is single-channel mono audio.
    Mono,

    /// This is stereo audio: 2 channels [l r].
    Stereo,

    /// This is some raw audio data without an interpretation.
    Raw { channels: NonZeroUsize },
}

impl ChannelFormat {
    pub fn get_channel_count(&self) -> NonZeroUsize {
        match self {
            ChannelFormat::Mono => NonZeroUsize::new(1).unwrap(),
            ChannelFormat::Stereo => NonZeroUsize::new(2).unwrap(),
            ChannelFormat::Raw { channels, .. } => *channels,
        }
    }
}

use super::*;
use crate::channel_format::ChannelFormat;

/// Knows how to read from a source.
///
/// The source may or may not be in the audio thread, but calling `read_samples` will always be audio-thread-safe if the
/// source is.
///
/// This tuype is what supports seeking and looping.  Resampling and where the source runs are built on top as other
/// types.
///
/// At the moment, we get off the ground by just always running in the audio thread. This is insufficient for the
/// finished implementation, which will need to call this logic in a background thread and funnel the samples across.
pub(crate) struct SampleSourceReader {
    source: Box<dyn SampleSource>,
    descriptor: Descriptor,

    source_state: SourceState,
}

#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Hash, Debug)]
enum SourceState {
    Playing,

    /// Set until the source seeks.
    AtEnd,

    /// Reached if the source errors or if the source says it's done forever.
    DoneForever,
}

impl SampleSourceReader {
    pub(crate) fn new(source: Box<dyn SampleSource>) -> Result<Self, SampleSourceError> {
        let descriptor = source.get_descriptor();

        Ok(Self {
            source,
            descriptor,
            source_state: SourceState::Playing,
        })
    }

    pub(crate) fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }

    /// Write interleaved audio data to the given slice.
    ///
    /// This function maty be called on the audio thread.  Un-interleaving is handled elsewhere.
    ///
    /// Returns the number of *frames* written.
    pub fn read_samples(&mut self, destination: &mut [f32]) -> Result<u64, SampleSourceError> {
        assert_eq!(
            destination.len() % self.descriptor.channel_format.get_channel_count().get(),
            0
        );

        if destination.is_empty() || self.source_state != SourceState::Playing {
            return Ok(0);
        }

        let wanted_frames =
            (destination.len() / self.descriptor.channel_format.get_channel_count().get()) as u64;

        match self.source.read_samples(destination) {
            Ok(x) if x == wanted_frames => Ok(x),
            Ok(x) => {
                assert!(x < wanted_frames);
                self.source_state = if self.source.is_permanently_finished() {
                    SourceState::DoneForever
                } else {
                    SourceState::AtEnd
                };
                Ok(x)
            }
            Err(e) => {
                // Can't log; may be on the audio thread.  We'll need to do something about this, but for now just what
                // that is going to be isn't clear.
                self.source_state = SourceState::DoneForever;
                Err(e)
            }
        }
    }
}

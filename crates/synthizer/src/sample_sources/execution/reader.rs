use crate::sample_sources::*;
use crate::LoopSpec;

use super::loop_driver::*;

/// Knows how to read from a source, including looping and seeking. Does not know how to buffer or resample.
///
/// The source may or may not be in the audio thread, but calling `read_samples` will always be audio-thread-safe if the
/// source is.
///
/// This tuype is what supports seeking and looping.  Resampling and where the source runs are built on top as other
/// types.
///
/// At the moment, we get off the ground by just always running in the audio thread. This is insufficient for the
/// finished implementation, which will need to call this logic in a background thread and funnel the samples across.
pub(crate) struct SourceReader {
    source: Box<dyn SampleSource>,
    descriptor: Descriptor,

    source_state: SourceState,

    loop_driver: LoopDriver,
}

#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Hash, Debug, derive_more::IsVariant)]
enum SourceState {
    /// The source is being driven by the loop driver.
    Playing,

    /// The source is sitting at eof. Until the user does something to change this, nothing happens.
    AtEof,

    /// Reached if the source errors or if the source says it's done forever.
    DoneForever,
}

impl SourceReader {
    pub(crate) fn new(source: Box<dyn SampleSource>) -> Result<Self, SampleSourceError> {
        let descriptor = source.get_descriptor();

        let loop_driver = LoopDriver::new(descriptor.sample_rate.get());

        Ok(Self {
            source,
            descriptor,
            source_state: SourceState::Playing,
            loop_driver,
        })
    }

    pub(crate) fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }

    /// Write interleaved audio data to the given slice.
    ///
    /// This function may be called on the audio thread.  Un-interleaving is handled elsewhere.
    ///
    /// Returns the number of *frames* written.  Returns 0 only for eof or the empty slice, but may return less than
    /// requested, particularly if the source hit a loop point.
    ///
    /// # Panics
    ///
    /// Panics if the slice's length is not a multiple of the channel count, since such slices cannot contain full
    /// frames.
    pub fn read_samples(&mut self, destination: &mut [f32]) -> Result<u64, SampleSourceError> {
        let got_frames = self.read_samples_impl(destination).inspect_err(|_| {
            self.source_state = SourceState::DoneForever;
        })?;
        if self.source.is_permanently_finished() {
            self.source_state = SourceState::DoneForever;
        } else if got_frames == 0 {
            self.source_state = SourceState::AtEof;
        }
        Ok(got_frames)
    }

    pub fn read_samples_impl(&mut self, destination: &mut [f32]) -> Result<u64, SampleSourceError> {
        assert_eq!(
            destination.len() % self.descriptor.channel_format.get_channel_count().get(),
            0
        );

        if destination.is_empty() || self.source_state != SourceState::Playing {
            return Ok(0);
        }

        let wanted_frames =
            (destination.len() / self.descriptor.channel_format.get_channel_count().get()) as u64;

        loop {
            let op = self.loop_driver.pre_read(wanted_frames);

            match op {
                ReadOp::Read(f) => {
                    let got = self.source.read_samples(
                        &mut destination[..f as usize * self.descriptor().get_channel_count()],
                    )?;
                    if got != 0 {
                        self.loop_driver.observe_read(got);
                        return Ok(got);
                    } else {
                        self.loop_driver.observe_eof();
                    }
                }
                ReadOp::Seek(p) => {
                    self.source.seek(p)?;
                    self.loop_driver.observe_seek(p);
                }
                ReadOp::ReachedEof => return Ok(0),
            }
        }
    }

    /// Move to playing if and only if at eof.
    ///
    /// We never want to move sources out of the permanently finished state, because that is stronger than eof.
    fn become_playing_if_eof(&mut self) {
        if self.source_state.is_at_eof() {
            self.source_state = SourceState::Playing;
        }
    }

    /// Configure seeking for this source by applying the loop specification.
    ///
    /// # Panics
    ///
    /// It is assumed that this specification was validated earlier, before getting as far as this driver, and that this
    /// is simply duplicating endpoint extraction.  If that is not the case, a panic results.
    pub(crate) fn config_looping(&mut self, spec: LoopSpec) {
        self.loop_driver
            .config_looping(spec)
            .expect("Loop specifications should have been validated earlier");
    }

    /// seek to the given sample, capped above by the descriptor's eof if any.
    pub(crate) fn seek(&mut self, new_pos: u64) -> Result<(), SampleSourceError> {
        let new_pos = self
            .descriptor
            .duration
            // A zero-length source is going to have weird behavior, but that's not on us.  Otherwise, we must not seek
            // past the end, and the end is `duration - 1`, like with arrays.  Seeks to 0 are required to always be
            // valid, but 0 for a 0-length source is past eof, so we break the tie by assuming that no one is going to
            // try to do that and will do our best to validate elsewhere.
            .map(|x| x.saturating_sub(1).min(new_pos))
            .unwrap_or(new_pos);
        self.source.seek(new_pos).inspect_err(|_| {
            self.source_state = SourceState::DoneForever;
        })?;
        self.loop_driver.observe_seek(new_pos);
        Ok(())
    }
}

use crate::error::Result;
use crate::loop_spec::*;

/// A computation engine for loops in the streaming use case
///
/// This type tells a caller when to seek versus when to continue reading, and how much to read.  It infers positions by
/// being given information on what the caller is doing, and then provides the necessary constraints on what the caller
/// can do next to enable looping.
///
/// In streaming contexts, it is not always the case that the underlying source is precise when seeking.  Additionally,
/// it is not always the case that the underlying source can cheaply read a specific frame.  This means two things.
/// First is that we do not need to interpolate between samples, since the inaccuracies we get from seeking
/// implementations are way worse than subsample accuracy.  Second is that we cannot definitively know when a loop
/// starts or ends via source-relative timestamps, because it is possible that the seek didn't go where we expect.  An
/// earlier attempt at all of this did try to ask sources to tell Synthizer where they went to, and we do only support
/// fixed-rate streams, but the decoding libraries weren't able to expose this information.
///
/// Instead, we rely on the fact that all codecs will seek to the same imprecise position when seeking inaccurately.
/// Even if this is not the case, it's close enough.  We can thus represent loops via maintaining the duration ourselves
/// and get something approximating what the user wants even on sources which cannot seek accurately.  Often, this is
/// only a few samples off.
///
/// The only remaining edge case is if a user seeks inaccurately somewhere else.  We can treat that as an accurate seek,
/// then eventually become consistent at our seek when the loop restarts.
///
/// Users which need perfect accuracy will need to either use accurate sources (we cannot currently provide this because
/// of limitations in symphonia), or switch to buffers.
///
/// The user-facing docs for all of this are on StreamingSourcePlayerNode, which provides more specific guarantees and
/// advice as to what does and doesn't work.
#[derive(Debug)]
pub(crate) struct LoopDriver {
    /// The pointer to the predicted position of the underlying source.
    predicted_position: u64,

    /// Are we currently trying to loop?
    looping: bool,

    /// Is this driver now at EOF?
    eof: bool,

    /// If looping, the start point of the loop.
    ///
    /// `Sample(0)` if not looping, or if the loop is the whole thing.
    loop_start: LoopPointInSamples,

    /// If looping, the endpoint of the source.  If not looping, `LoopPointInSamples::End`.
    ///
    /// As per docs on [LoopSpec], this is inclusive.
    loop_end: LoopPointInSamples,

    /// The sample rate of the underlying source, used to convert user-provided loop specifications.
    sr: u64,
}

#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) enum ReadOp {
    Read(u64),
    Seek(u64),
    ReachedEof,
}

impl LoopDriver {
    /// Create a driver which does not loop.
    pub(crate) fn new(sr: u64) -> Self {
        Self {
            predicted_position: 0,
            looping: false,
            eof: false,
            loop_start: LoopPointInSamples::Sample(0),
            loop_end: LoopPointInSamples::End,
            sr,
        }
    }

    /// Configure this driver with a given loop specification.
    pub(crate) fn config_looping(&mut self, config: LoopSpec) -> Result<()> {
        let Some((start, end)) = config.get_endpoints_samples(self.sr)? else {
            self.looping = false;
            self.loop_start = LoopPointInSamples::Sample(0);
            self.loop_end = LoopPointInSamples::End;
            return Ok(());
        };

        self.loop_start = start;
        self.loop_end = end;
        self.looping = true;

        Ok(())
    }

    /// Tell the driver that the caller wishes to read up to the specified number of frames, then let it tell the caller
    /// what to do.
    pub(crate) fn pre_read(&mut self, frames: u64) -> ReadOp {
        // Really simple case: if we are not looping, then do nothing special; the user may simply do what they asked to
        // be able to do.
        if !self.looping {
            if self.eof {
                return ReadOp::ReachedEof;
            } else {
                return ReadOp::Read(frames);
            }
        }

        // Otherwise, if we are at eof, then the user must immediately seek to the loop's start point.
        if self.eof {
            return ReadOp::Seek(self.loop_start_in_samples());
        }

        // Otherwise, we need to clamp what the user may do, then return eitherb a read op if there is data to read or a
        // seek op if a seek is necessary.
        match self.loop_end {
            // in this case we read until EOF is observed, then EOF is handled above.
            LoopPointInSamples::End => ReadOp::Read(frames),
            LoopPointInSamples::Sample(wanted_end) => {
                // Remember it's inclusive.  No one really cares about u64::MAX, but this handles it anyway.
                let avail = if self.predicted_position > wanted_end {
                    0
                } else {
                    // + 1 because wanted_end is inclusive.
                    wanted_end - self.predicted_position + 1
                };
                let clamped = frames.min(avail);

                if clamped == 0 {
                    ReadOp::Seek(self.loop_start_in_samples())
                } else {
                    ReadOp::Read(clamped)
                }
            }
        }
    }

    fn loop_start_in_samples(&self) -> u64 {
        match self.loop_start {
            LoopPointInSamples::Sample(x) => x,
            LoopPointInSamples::End => unreachable!("Loop start points are always at a sample"),
        }
    }

    /// Observe an eof condition from the parent.
    pub(crate) fn observe_eof(&mut self) {
        self.eof = true;

        // If the predicted position is not after the loop start, then stop looping; we can't perform a loop from eof to
        // eof.  For inaccurate sources, the requirement that seeking must be early should prevent this from happening.
        if self.predicted_position < self.loop_start_in_samples() {
            self.looping = false;
        }
    }

    /// Observe a seek, which will move the position to the new value.
    pub(crate) fn observe_seek(&mut self, new_pos: u64) {
        self.predicted_position = new_pos;
        self.eof = false;
    }

    /// Observe a read operation.
    ///
    /// This must never be called with a value larger than obtained from a [ReadOp].
    pub(crate) fn observe_read(&mut self, amount: u64) {
        self.predicted_position += amount;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test just moving a source from the start to the end.
    #[test]
    fn test_not_looping() {
        let mut driver = LoopDriver::new(1000);
        assert_eq!(driver.pre_read(10), ReadOp::Read(10),);
        driver.observe_read(10);
        assert_eq!(driver.predicted_position, 10);
        assert_eq!(driver.pre_read(10), ReadOp::Read(10),);
        driver.observe_read(10);
        assert_eq!(driver.predicted_position, 20);
        driver.observe_eof();
        assert_eq!(driver.pre_read(10), ReadOp::ReachedEof,);
        // Seeks undo eof.
        driver.observe_seek(10);
        assert_eq!(driver.pre_read(10), ReadOp::Read(10),);
    }

    #[test]
    fn test_full_loop() {
        let mut driver = LoopDriver::new(1000);
        driver.config_looping(LoopSpec::all()).unwrap();

        for _ in 0..2 {
            assert_eq!(driver.pre_read(10), ReadOp::Read(10),);
            driver.observe_read(10);
            assert_eq!(driver.predicted_position, 10);
            assert_eq!(driver.pre_read(10), ReadOp::Read(10),);
            driver.observe_read(10);
            assert_eq!(driver.predicted_position, 20);
            driver.observe_eof();
            assert_eq!(driver.pre_read(10), ReadOp::Seek(0), "{driver:?}");
            driver.observe_seek(0);
        }
    }

    /// Test a loop on a 20-sample source from samples 15 to 18 inclusive.
    #[test]
    fn test_partial_loop() {
        let mut driver = LoopDriver::new(1000);
        driver
            .config_looping(LoopSpec::samples(15, Some(18)))
            .unwrap();

        // This puts us in the loop at sample 15, the loop's start point.
        assert_eq!(driver.pre_read(15), ReadOp::Read(15));
        driver.observe_read(15);

        // The first, simplest, case is to read some samples one by one.  We have 4 samples, then a seek.
        for _ in 0..4 {
            assert_eq!(driver.pre_read(1), ReadOp::Read(1), "{driver:?}",);
            driver.observe_read(1);
        }

        // Then this should want to seek.
        assert_eq!(driver.pre_read(1), ReadOp::Seek(15), "{driver:?}",);
        driver.observe_seek(15);

        // The other case of looping is an attempt to read the whole loop at once, or potentially more.
        assert_eq!(driver.pre_read(10), ReadOp::Read(4),);
        driver.observe_read(4);
        assert_eq!(driver.pre_read(10), ReadOp::Seek(15));
    }

    /// When sources are configured to loop after EOF, they should never seek and simply stop at EOF.
    #[test]
    fn test_after_eof() {
        let mut driver = LoopDriver::new(10000);
        driver.config_looping(LoopSpec::samples(30, None)).unwrap();

        // Read 20 samples. Then eof it.
        assert_eq!(driver.pre_read(20), ReadOp::Read(20),);
        driver.observe_read(20);
        driver.observe_eof();

        // And then nothin should ever be able to happe again.
        assert_eq!(driver.pre_read(10), ReadOp::ReachedEof,);
    }
}

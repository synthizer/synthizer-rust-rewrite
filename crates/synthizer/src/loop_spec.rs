use std::time::Duration;

/// Specification of a loop.
///
/// When Synthizer is asked to loop, this type tells it how.  The helper methods on it build specified loop
/// configurations.  The following are the available options:
///
/// - None, configured with [LoopSpec::no_looping].  Disables looping on whatever it is applied to.
/// - All, configured with [LoopSpec::all].  Loop over the entire audio, start to finish.
/// - Samples, configured with [LoopSpec::samples].  Loop over the specified subslice of the audio.  The endpoints are
///   inclusive.
/// - Timestamps, configured with [LoopSpec::timestamps].  Given [Duration]s from the beginning of the audio, attempt to
///   interpolate to produce an exact loop, even if the endpoints don't fall on specific samples.  The endpoints are
///   inclusive.
///
/// IMPORTANT: the endpoints are inclusive because the timestamp use case is effectively real numbers.  If you need
/// exclusive ranges you must handle this yourself, probably by using samples and subtracting from the endpoint.
///
/// In the case that the underlying node does not support looping with interpolation, timestamp loops are rounded down
/// to the nearest sample.  This rounding choice is made to avoid unpredictable issues around infinitecimals, since
/// round-to-nearest is somewhat flaky in the sense that even a microsecond one way or the other can cause unpredictable
/// changes.  It is possible to extend Synthizer to specify rounding behaviors here given a good enough reason, so feel
/// free to open an issue if you think you have one to tell us what that reason is.
///
/// If any given loop configuration would produce an empty loop or if the endpoint is before the start, an error results
/// at the point of usage (not the point of building), which is done so that Synthizer has a chance to round the loop
/// off if such rounding is going to be necessary.  This may be detected by calling [Error::is_invalid_loop] on the
/// Synthizer error returned, though it should be noted that for most applications this is a fatal condition.  For
/// practical purposes, loops larger than 0.5ms will never error even when rounded for all reasonable audio
/// applications.  For the mathematical the constraint is that all loops whose duration are longer than `1.0/sr` seconds
/// are valid, where `sr` is the sample rate of the source audio.  It should be noted that loops one sample long will
/// introduce DC artifacts since the position can never advance significantly, and that loops only a few samples long
/// which don't introduce DC artifacts probably introduce aliasing instead unless carefully constructed to meet the
/// nyquist criterion.  These are true of all audio libraries, not just Synthizer, as they are limitations of digital
/// audio itself.
///
/// As a non-math simplification of the above: if you don't know what you're doing and you're not trying to write a
/// synthesizer, you probably don't want loops under 100ms or so because they will sound like waveforms, and those
/// waveforms will probably be aliased and full of artifacts.
///
/// Also note that loops based on samples (not seconds) may be accelerated, particularly when using buffers without
/// playback rate changes.  This is a proof to Synthizer that it may avoid interpolation, and often turns into simple
/// memcpys internally.  Audio artists can provide the sample-based positions in the original audio file if using loops
/// in the middle of content.  If using seconds, it is possible to call [LoopSpec::allow_rounding] to indicate that it
/// is okay to round loops off.  This is not a guarantee that such rounding will occur nor does it guarantee what such
/// rounding will do.  It only tells Synthizer it may do so if it feels that it can get an optimization out of it.  Said
/// rounding will only be off by a couple samples at most and occurs only to timestamp-based loops.  This is useful for
/// things such as background music where subsample interpolation isn't worth the cost, but only timestamps are
/// available, e.g. because files are transcoded via some process in an art packaging pipeline.
///
/// The `Default` impl is the same as no looping.
#[derive(Copy, Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct LoopSpec {
    kind: LoopSpecKind,
    allow_rounding: bool,
}

#[derive(Copy, Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
enum LoopSpecKind {
    #[default]
    None,
    All,
    Samples {
        start: u64,
        end: Option<u64>,
    },
    Timestamps {
        start: Duration,
        end: Option<Duration>,
    },
}

#[derive(thiserror::Error, Debug, Clone)]
pub(crate) enum LoopSpecError {
    #[error("Attempt to create a loop which is empty. Specification is {0:?} and the sampling rate is {1}")]
    EmptyLoop(LoopSpec, u64),

    #[error("Attempt to create a loop whose endpoint is before the beginning with spec {0:?} against sampling rate {1}")]
    EndBeforeBeginning(LoopSpec, u64),

    #[error("Attempt to create a loop whose start point is after the end of the audio")]
    StartAfterEof,

    #[error("Attempt to create a loop whose endpoint is after the end of the audio")]
    EndAfterEof,
}

/// This internal type allows comparing loop points to u64s.
///
/// It is basically Option, except that the `None` variant is greater than all possible values.
///
/// No loop is represented by `None` as in `Option<T>`.  The loop over all of something is `Sample(0)` to `End`.  The
/// loop over part of something is `Sample` as both endpoints.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) enum LoopPointInSamples {
    Sample(u64),
    End,
}

impl LoopSpec {
    /// Don't loop at all.
    pub fn none() -> Self {
        Self {
            kind: LoopSpecKind::None,
            allow_rounding: false,
        }
    }

    /// Loop over the whole audio, start to finish.
    pub fn all() -> Self {
        Self {
            kind: LoopSpecKind::All,
            allow_rounding: false,
        }
    }

    /// Loop over the specified subslice, specified as samples in the sample rate of the backing audio.
    ///
    /// For example, a source at 22050 HZ is not Synthizer's internal sampling rate, and so these endpoints would be
    /// relative to 22050 HZ.
    ///
    /// If `end` is `None`, the loop starts at start and proceeds to the end of the audio.
    pub fn samples(start: u64, end: Option<u64>) -> Self {
        Self {
            kind: LoopSpecKind::Samples { start, end },
            allow_rounding: false,
        }
    }

    /// Loop over the specified slice of audio, specified in timestamps relative to the start.
    ///
    /// If `end` is `None`, loop from the start to the end of the underlying audio.
    pub fn timestamps(start: Duration, end: Option<Duration>) -> Self {
        Self {
            kind: LoopSpecKind::Timestamps { start, end },
            allow_rounding: false,
        }
    }

    /// take this specification and return a new one which allows rounding.
    #[must_use]
    fn allow_rounding(mut self) -> Self {
        self.allow_rounding = true;
        self
    }

    /// Internal.  Take this specification and convert it to one which is not using timestamp based loops by rounding
    /// the timestamps to the nearest sample.
    pub(crate) fn force_round_off(self, sr: u64) -> Self {
        let new_kind = match self.kind {
            x @ (LoopSpecKind::None | LoopSpecKind::All | LoopSpecKind::Samples { .. }) => x,
            LoopSpecKind::Timestamps { start, end } => {
                let start_sr = (start.as_secs_f64() * sr as f64).floor() as u64;
                let end_sr = end.map(|x| (x.as_secs_f64() * sr as f64).floor() as u64);
                LoopSpecKind::Samples {
                    start: start_sr,
                    end: end_sr,
                }
            }
        };

        Self {
            kind: new_kind,
            allow_rounding: false, // we won't round again.
        }
    }

    /// Get this loop's start and endpoint in samples, if any.
    ///
    /// This requires rounding off.
    pub(crate) fn get_endpoints_samples(
        &self,
        sr: u64,
    ) -> Result<Option<(LoopPointInSamples, LoopPointInSamples)>, LoopSpecError> {
        let rounded = self.force_round_off(sr);
        let ret = match rounded.kind {
            LoopSpecKind::Timestamps { .. } => unreachable!("We just rounded"),
            LoopSpecKind::None => None,
            LoopSpecKind::All => Some((LoopPointInSamples::Sample(0), LoopPointInSamples::End)),
            LoopSpecKind::Samples { start, end } => Some((
                LoopPointInSamples::Sample(start),
                end.map(LoopPointInSamples::Sample)
                    .unwrap_or(LoopPointInSamples::End),
            )),
        };

        if let Some((x, y)) = ret {
            use std::cmp::Ordering;
            match x.cmp(&y) {
                Ordering::Equal => return Err(LoopSpecError::EmptyLoop(*self, sr)),
                Ordering::Greater => return Err(LoopSpecError::EndBeforeBeginning(*self, sr)),
                _ => {}
            }
        }

        Ok(ret)
    }

    /// Validate this specification against a duration in samples, if one is known.
    pub(crate) fn validate(
        &self,
        sr: u64,
        duration_in_samples: Option<u64>,
    ) -> Result<(), LoopSpecError> {
        // Regardless of anything else, it should be possible to get sample endpoints; we do not permit loops of less than one sample.
        self.get_endpoints_samples(sr)?;

        let Some(dur_samples) = duration_in_samples else {
            return Ok(());
        };

        // Whole seconds.
        let secs = dur_samples / sr;

        // Subseconds in samples.
        let rem = dur_samples % sr;

        // We want nanoseconds, but rounded up because of roundig errors; we can give users a little bit of slop
        // especially in the case of buffers where things do not go to zero immediately if interpolating.  Frankly, no
        // one will ever notice an extra nanosecond, but they may notice a missing one if they are writing tests where
        // endpoints perfectly match up in odd sample rates.
        //
        //  This is (rem / sr * NS) except that we wish to avoid floating point values; to do so, rearrange as (rem * NS
        //  / sr).  rem * NS can be computed directly and accurately; dividing out by sr needs to round up.
        let nanos:u32 = (rem * 1000000).div_ceil(sr).try_into().expect("If we got more than a billion here, we somehow concluded that less than a sample of seconds is more than a second");

        let got_dur = Duration::new(secs, nanos);

        match self.kind {
            LoopSpecKind::None | LoopSpecKind::All => Ok(()),
            LoopSpecKind::Samples { start, end } => {
                let end = end.unwrap_or(0).saturating_sub(1);
                // These are inclusive, so watch out for that.
                if start >= dur_samples - 1 {
                    return Err(LoopSpecError::StartAfterEof);
                } else if end >= dur_samples {
                    return Err(LoopSpecError::EndAfterEof);
                }

                Ok(())
            }
            LoopSpecKind::Timestamps { start, end } => {
                let end = end.unwrap_or(Duration::from_secs(0));
                // These are also inclusive, but durations are infinite and we can't subtract anything meaningful. Just
                // let it be, and we already handle the extreme case because looping for sources with durations and
                // without is identical.
                if start >= got_dur {
                    return Err(LoopSpecError::StartAfterEof);
                } else if end >= got_dur {
                    return Err(LoopSpecError::EndAfterEof);
                }

                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_samples() {
        use LoopPointInSamples::*;

        assert_eq!(LoopSpec::none().get_endpoints_samples(10000).unwrap(), None);
        assert_eq!(
            LoopSpec::all().get_endpoints_samples(10000).unwrap(),
            Some((Sample(0), End)),
        );
        assert_eq!(
            LoopSpec::samples(5, None)
                .get_endpoints_samples(10000)
                .unwrap(),
            Some((Sample(5), End))
        );
        assert_eq!(
            LoopSpec::samples(5, Some(15))
                .get_endpoints_samples(10000)
                .unwrap(),
            Some((Sample(5), Sample(15))),
        );
        assert_eq!(
            LoopSpec::timestamps(Duration::from_secs(1) + Duration::from_millis(1), None)
                .get_endpoints_samples(100)
                .unwrap(),
            Some((Sample(100), End))
        );
        assert_eq!(
            LoopSpec::timestamps(
                Duration::from_secs(1) + Duration::from_millis(1),
                Some(Duration::from_secs(3) + Duration::from_millis(1))
            )
            .get_endpoints_samples(100)
            .unwrap(),
            Some((Sample(100), Sample(300))),
        );
    }
}

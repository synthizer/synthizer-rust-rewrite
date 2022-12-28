use std::iter::IntoIterator;

/// A clock, measuring time in samples.
///
/// Clocks return [SampleTimestamp]s, which may be converted to f64 seconds but, by default, point at samples.
///
/// Every clock has an associated sample rate, which is the amount of time calls to [SampleClock::tick] will advance the
/// clock by in seconds (internally this is one sample).
///
/// Clocks are also iterators, yielding [SampleTimestamp]s.
///
/// Clocks cannot go backward by design.
#[derive(Debug)]
pub struct SampleClock {
    pub sample_time: u64,
    pub sr: f64,
}

/// A timestamp in samples.
///
/// These do not preserve the sample rate.  Two timestamps are ordered only with respect to the raw sample index.
/// hashing is also off only the raw sample index.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct SampleTimestamp {
    sample_time: u64,
}

impl SampleClock {
    /// Create a clock at time zero for a given samplerate.
    pub fn new(sr: f64) -> SampleClock {
        SampleClock { sample_time: 0, sr }
    }

    /// Get the current time.
    pub fn now(&self) -> SampleTimestamp {
        SampleTimestamp {
            sample_time: self.sample_time,
        }
    }

    /// Advance this clock by one sample.
    pub fn tick(&mut self) {
        // tick_multiple has a debug assert, so don't use it.
        self.sample_time += 1;
    }

    /// Advance this clock by multiple samples.
    ///
    /// # Panics
    ///
    /// Panics in debug builds only if the increment is zero.
    pub fn tick_multiple(&mut self, increment: u64) {
        debug_assert!(
            increment != 0,
            "Attempt to advance a clock by no time at all"
        );
        self.sample_time += increment;
    }

    /// Create a clock which shares the same time as this one, but which will advance separately.
    pub fn fork(&self) -> SampleClock {
        SampleClock {
            sample_time: self.sample_time,
            sr: self.sr,
        }
    }

    /// Iterate over this clock, advancing the timestamp as we go.
    ///
    /// The first value of the iterator is always now, and the clock will be advanced to 1 past the end of the iterator.
    /// This is done to match up with arrays: if you advance by 5 items, then you get `0..5` and the iterator is on 6
    /// next time.
    pub fn iter(&'_ mut self) -> impl Iterator<Item = SampleTimestamp> + '_ {
        std::iter::from_fn(move || {
            let now = self.now();
            self.tick();
            Some(now)
        })
    }

    /// Iterate over this clock, but return a range of usize which starts at 0.
    ///
    /// This can be used when iterating over arrays where the time is only important in that it advances.
    pub fn iter_usize<'a, R>(&'a mut self, range: R) -> impl Iterator<Item = usize> + '_
    where
        R: std::ops::RangeBounds<usize> + IntoIterator<Item = usize> + 'a,
    {
        self.tie_iter(range.into_iter())
    }

    /// Tie an iterator to this clock, such that advancing the iterator advances the clock as well.
    ///
    ///
    pub fn tie_iter<'a, Iter>(
        &'a mut self,
        iterator: Iter,
    ) -> impl Iterator<Item = <Iter as Iterator>::Item> + 'a
    where
        Iter: Iterator + 'a,
    {
        iterator.map(|x| {
            self.tick();
            x
        })
    }
}

impl SampleTimestamp {
    pub fn get_sample(&self) -> u64 {
        self.sample_time
    }

    pub fn to_seconds(&self, assumed_sr: f64) -> f64 {
        self.sample_time as f64 / assumed_sr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let mut clock = SampleClock::new(44100.0);
        clock.tick();
        assert_eq!(clock.sample_time, 1);
        clock.tick_multiple(5);
        assert_eq!(clock.sample_time, 6);
        assert_eq!(clock.now().get_sample(), 6);
    }
    #[test]
    fn test_forking() {
        let mut clock = SampleClock::new(44100.0);
        let clock2 = clock.fork();
        clock.tick();
        assert_eq!(clock.sample_time, 1);
        assert_eq!(clock2.sample_time, 0);
    }

    #[test]
    fn test_iterator_tying() {
        let iterator = 0..5u64;
        let mut clock = SampleClock::new(44100.0);
        for _ in clock.tie_iter(iterator) {}
        assert_eq!(clock.sample_time, 5);
    }
}

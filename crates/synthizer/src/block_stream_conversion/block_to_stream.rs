use crate::channel_format::ChannelFormat;
use crate::config::*;

/// Converts from a block of audio data on the left, to a stream of audio data on the right.
///
/// The left is usually something internal to Syntizer, and the right something external such as an audio device or a
/// user requesting samples.
///
/// Notably, because of the nature of this type to "pierce" an abstraction, this works on slices on both sides. This
/// also gets around Rust not supporting templated closures; we cannot hide the fact that a slice is involved anyway,
/// while monomorphizing instead of using dynamic dispatch.
pub(crate) struct BlockToStreamConverter {
    block: [f32; CHANNEL_BLOCK_LEN],
    available_frames: usize,
    format: ChannelFormat,
}

impl BlockToStreamConverter {
    pub(crate) fn new(format: ChannelFormat) -> Self {
        Self {
            block: [0.0; CHANNEL_BLOCK_LEN],
            available_frames: 0,
            format,
        }
    }

    /// Fill a slice with some available data, draining the available data we still have.
    ///
    /// Returns how many frames were written out.
    pub(crate) fn drain_once(&mut self, mut destination: &mut [f32]) -> usize {
        let chan_count = self.format.get_channel_count().get();
        assert_eq!(destination.len() % chan_count, 0);

        let needed = destination.len() / chan_count;
        let will_write = self.available_frames.min(needed);
        let src_start = (BLOCK_SIZE - self.available_frames) * chan_count;
        let src_end = src_start + will_write * chan_count;
        destination[..will_write * chan_count].copy_from_slice(&self.block[src_start..src_end]);
        self.available_frames -= will_write;
        will_write
    }

    /// Fill a slice with audio data.
    ///
    /// The slice length must be a multiple of the channel format (zero-length slices are allowed).  The provided
    /// callback must fill one block of audio given a slice `BLOCK_SIZE * channels` in length.
    pub(crate) fn fill_slice(
        &mut self,
        mut destination: &mut [f32],
        mut refill: impl FnMut(&mut [f32]),
    ) {
        let chan_count = self.format.get_channel_count().get();
        assert_eq!(destination.len() % chan_count, 0);

        while !destination.is_empty() {
            if self.available_frames == 0 {
                refill(&mut self.block[..chan_count * BLOCK_SIZE]);
                self.available_frames = BLOCK_SIZE;
            }

            let move_by = self.drain_once(destination);
            assert!(move_by != 0);
            destination = &mut destination[move_by * chan_count..];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test by filling the internal block with an incrementing counter, then see if we get an incrementing sequence
    /// back out given some weird values to increment by.
    #[test]
    fn test_refilling_by_counter() {
        for format in [ChannelFormat::Mono, ChannelFormat::Stereo] {
            let mut counter = 0.0f32;
            let mut seen_counter = 0.0f32;
            let num_chan = format.get_channel_count().get();
            let mut conv = BlockToStreamConverter::new(format);

            let mut buf = vec![];
            let mut wrote = 0;
            let mut increments = [1, 2, 123, 1024, 4096, 123456].into_iter().cycle();
            while wrote < 1 << 20 {
                let incr = increments.next().unwrap();
                buf.resize(incr * num_chan, 0.0f32);
                conv.fill_slice(&mut buf[..], |slice| {
                    for s in slice.iter_mut() {
                        *s = counter;
                        counter += 1.0f32;
                    }
                });

                wrote += num_chan * incr;

                for (i, v) in buf.iter().copied().enumerate() {
                    assert_eq!((i, v), (i, seen_counter));
                    seen_counter += 1.0;
                }
            }
        }
    }
}

use crate::data_structures::RefillableWrapper;
use crate::loop_spec::LoopSpec;
use crate::sample_sources::{Descriptor, SampleSourceError};

use super::reader::SourceReader;

/// Un-tunable size of a buffered source's buffer, in frames.
const BUFSIZE: usize = 8192;

/// Wraps a SampledSourceReader with a buffer, so that we do not continually attempt to drain small amounts out of a
/// source, and handles partial reads.
///
/// Using this type, exactly as many samples as requested will be produced, or the underlying source was at the end.
///
/// The buffer is of fixed size, see [BUFSIZE].
pub(crate) struct BufferedSourceReader {
    reader: SourceReader,
    buffer: RefillableWrapper<Vec<f32>>,
}

impl BufferedSourceReader {
    pub(crate) fn new(reader: SourceReader) -> Self {
        let desc = reader.descriptor();
        let chans = desc.channel_format.get_channel_count().get();
        let backing = vec![0.0f32; chans * BUFSIZE];
        let buffer = RefillableWrapper::new(backing);

        Self { reader, buffer }
    }

    pub(crate) fn get_reader(&self) -> &SourceReader {
        &self.reader
    }

    pub(crate) fn get_reader_mut(&mut self) -> &mut SourceReader {
        &mut self.reader
    }

    pub fn read_samples(&mut self, destination: &mut [f32]) -> Result<u64, SampleSourceError> {
        let chans = self
            .get_reader()
            .descriptor()
            .channel_format
            .get_channel_count()
            .get();
        assert_eq!(destination.len() % chans, 0);
        let frames = (destination.len() / chans) as u64;

        let mut done = 0u64;

        // True if the buffer could be refilled, but not completely.  This means the underlying source has reached the
        // end and any data we have is the end.
        let mut has_seen_partial = false;

        while done < frames {
            let offset = done as usize * chans;
            let got_this_time = self.drain_buffer(&mut destination[offset..]);
            done += got_this_time;

            // If there is nothing in the buffer and additionally we got a partial refill last time, that's it; the
            // source can't be refilled further until such time as the user does something like seeking.
            if got_this_time == 0 && has_seen_partial {
                break;
            }

            // Avoid refills when not needed, as this can introduce latency if things line up so that we exactly
            // fulfilled the request.
            //
            // Also avoid refills if we got a partial refill last time because, pending user action, that's it for this
            // source.
            if done < frames || has_seen_partial {
                has_seen_partial = self.refill_buffer()? != BUFSIZE as u64;
            }
        }
        Ok(done)
    }

    /// Drain the buffer into destination, returning how much was drained.
    fn drain_buffer(&mut self, destination: &mut [f32]) -> u64 {
        let slice = self.buffer.consume_start();
        let can_do = destination.len().min(slice.len());
        (destination[..can_do]).copy_from_slice(&slice[..can_do]);
        self.buffer.consume_end(can_do);
        (can_do / self.reader.descriptor().get_channel_count()) as u64
    }

    /// Drive the underlying source to fill as much of the buffer as possible.
    ///
    /// This is only ever called after the buffer is completely empty so, as a result, it always tries to fill the whole
    /// buffer.
    fn refill_buffer(&mut self) -> Result<u64, SampleSourceError> {
        let chans = self.reader.descriptor().get_channel_count();
        let bufdest = self
            .buffer
            .refill_start_all()
            .expect("This buffer should be empty");
        assert_eq!(bufdest.len() % chans, 0);

        let mut got_total = 0u64;
        let needed_frames = (bufdest.len() / chans) as u64;

        while got_total < needed_frames {
            let offset = got_total as usize * chans;
            let partial_dest = &mut bufdest[offset..];
            let got_now = self.reader.read_samples(partial_dest)?;
            if got_now == 0 {
                break;
            }
            got_total += got_now;
        }

        self.buffer.refill_end((got_total as usize) * chans);

        Ok(got_total)
    }

    pub(crate) fn descriptor(&self) -> &Descriptor {
        self.get_reader().descriptor()
    }

    /// Set the underlying source to loop.
    pub(crate) fn config_looping(&mut self, spec: LoopSpec) {
        self.reader.config_looping(spec);
    }

    /// Try to seek the underlying source.
    ///
    /// if successful, this clears the buffer.  The underlying driver knows how to end sources forever on seek errors,
    /// so we needn't handle that here.
    pub(crate) fn seek(&mut self, new_pos: u64) -> Result<(), SampleSourceError> {
        self.reader.seek(new_pos)?;
        self.buffer.reset();
        Ok(())
    }
}

use std::cell::RefCell;

use crate::config::*;
use crate::data_structures::SplittableBuffer;

use super::buffered::BufferedSourceReader;
use super::reader::SourceReader;
use super::resampler::Resampler;
use crate::sample_sources::{SampleSource, SampleSourceError};

/// Drive a source of samples.
///
/// This type handles things like optional resampling.
///
/// Callers want to go one more level up and use the executor, which is capable of also handling background thread
/// management.  There are no really good names for these, sadly.
pub(crate) struct Driver {
    kind: SampleSourceDriverKind,
}

#[allow(clippy::large_enum_variant)]
enum SampleSourceDriverKind {
    Buffered(BufferedSourceReader),
    Resampled(Resampler),
}

impl Driver {
    pub(crate) fn new<S: SampleSource>(source: S) -> Result<Self, SampleSourceError> {
        // There is always a reader.
        let reader = SourceReader::new(Box::new(source))?;

        // There is also always a buffer.
        let buffered = BufferedSourceReader::new(reader);

        let src_sr = buffered.descriptor().sample_rate.get();

        // Where we diverge is if there is a resampler.
        let kind = if src_sr == crate::config::SR as u64 {
            SampleSourceDriverKind::Buffered(buffered)
        } else {
            let resampled = Resampler::new(buffered)?;
            SampleSourceDriverKind::Resampled(resampled)
        };

        Ok(Self { kind })
    }

    /// Read one block of samples from the underlying engine, un-interleaving them into the specified slice.
    ///
    /// If there are less available than a block's worth, zero them out.  That should only happen at the end if the
    /// source isn't looping.
    ///
    /// Returns how many frames were written.  Due to internal implementation details this can only be less than
    /// `BLOCK_SIZE` in the case that no resampling is going on or the source is at the end.  Any unwritten data is
    /// filled with zeros.
    pub(crate) fn read_samples(
        &mut self,
        destination: &mut [f32],
    ) -> Result<u64, SampleSourceError> {
        // If we aren't resampling, we must round-trip through an intermediate block to uninterleave. Rather than
        // allocating those all over, thread locals are suitable.
        const SIZE: usize = BLOCK_SIZE * MAX_CHANNELS;
        thread_local! {
            static TMP_BUF: RefCell<[f32; SIZE]> = const { RefCell::new([0.0f32; SIZE]) };
        }

        match &mut self.kind {
            SampleSourceDriverKind::Resampled(r) => {
                // In this case, we can go directly to the destination and we're done.
                r.read_samples(destination)?;
                Ok(BLOCK_SIZE as u64)
            }
            SampleSourceDriverKind::Buffered(r) => {
                // In this case, we must go via the temporary thread-local buffer.  Because the destination slice is
                // un-interleaved, we will un-interleave the whole buffer which handles getting the zeros in the right
                // place.
                TMP_BUF.with(|tmp_buf| -> Result<u64, SampleSourceError> {
                    let mut tmp_buf = tmp_buf.borrow_mut();

                    let chans = r.descriptor().get_channel_count();
                    let slice = &mut tmp_buf[0..(chans * BLOCK_SIZE)];
                    let got = r.read_samples(slice)?;
                    slice[(got as usize * chans)..].fill(0.0f32);

                    let mut splittable = SplittableBuffer::new(
                        destination,
                        r.descriptor().channel_format.get_channel_count(),
                    );
                    let mut split = splittable.split_mut();

                    for c in 0..chans {
                        let d = &mut split[c];
                        for i in 0..BLOCK_SIZE {
                            let src_ind = chans * i + c;
                            d[i] = tmp_buf[src_ind];
                        }
                    }

                    Ok(got)
                })
            }
        }
    }

    pub(crate) fn descriptor(&self) -> &crate::sample_sources::Descriptor {
        match &self.kind {
            SampleSourceDriverKind::Resampled(r) => r.descriptor(),
            SampleSourceDriverKind::Buffered(r) => r.descriptor(),
        }
    }

    /// Config the underlying source to loop.
    ///
    /// This assumes that the spec has already been validated. Furthermore, the sample rate of the loop endpoints are in
    /// the sample rate of the ultimately underlying source, not that of Synthizer.
    pub(crate) fn config_looping(&mut self, spec: crate::LoopSpec) {
        match &mut self.kind {
            SampleSourceDriverKind::Buffered(x) => x.config_looping(spec),
            SampleSourceDriverKind::Resampled(x) => x.config_looping(spec),
        }
    }

    pub(crate) fn seek(&mut self, new_pos: u64) -> Result<(), SampleSourceError> {
        match &mut self.kind {
            SampleSourceDriverKind::Buffered(b) => b.seek(new_pos),
            SampleSourceDriverKind::Resampled(r) => r.seek(new_pos),
        }
    }
}

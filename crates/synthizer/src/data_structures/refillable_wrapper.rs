//! A wrapper which may be placed around various buffer-like things to support draining and refilling them.
//!
//! This wrapper can take anything implementing [RefillableWrapped] and turn it into a buffer which may be refilled
//! using the helper methods to consume and refill data.
//!
//! This is generic over what would otherwise be a bunch of one-off things wrapping a buffer of some sort with a
//! counter.
use std::num::NonZeroUsize;
use std::ops::Range;

/// A trait representing something that can be wrapped by [RefillableWrapper].
///
/// The simplest implementation of this trait is for `Vec`, which is written below.  If implementing this, consult that
/// for an example of what to do.
pub(crate) trait RefillableWrapped {
    type Sliced<'a>
    where
        Self: 'a;
    type SlicedMut<'a>
    where
        Self: 'a;

    /// "slice" this wrapped buffer, returning all of the data.
    ///
    /// For example, with a vec this is just a slice. With SplittableBuffer, it's the output of having split it, etc.
    fn slice(&self, range: Range<usize>) -> Self::Sliced<'_>;

    fn slice_mut(&mut self, range: Range<usize>) -> Self::SlicedMut<'_>;

    /// Return the length of the underlying buffer.
    fn len(&self) -> usize;

    /// Copy the given range to the beginning of the buffer.
    fn copy_to_beginning(&mut self, range: Range<usize>);
}

impl<T: Copy + Default + 'static> RefillableWrapped for Vec<T> {
    type Sliced<'a> = &'a [T];
    type SlicedMut<'a> = &'a mut [T];

    fn slice(&self, range: Range<usize>) -> Self::Sliced<'_> {
        &self[range]
    }

    fn slice_mut(&mut self, range: Range<usize>) -> Self::SlicedMut<'_> {
        &mut self[range]
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn copy_to_beginning(&mut self, range: Range<usize>) {
        self.copy_within(range, 0);
    }
}

/// A wrapper which can keep track of how much of a buffer is used.
///
/// The wrapper may only hold up to the length of the internal buffer, which cannot be grown or directly accessed after
/// wrapping.  Callers should create the internal buffer themselves with the desired length before wrapping.
///
/// Usage is to use `refill_start` and `refill_end` to fill the buffer up if needed, then `consume_start` and
/// `consume_end` to read it.  We avoid closures because closures tend to lead to borrow checker fun should this API
/// ever be extended.
pub(crate) struct RefillableWrapper<Buffer: RefillableWrapped> {
    buffer: Buffer,

    /// The data is represented as a valid range start..end, where start moves forward as data is consumed and end moves
    /// forward as data is produced.
    start: usize,

    /// See doc comment on start.
    end: usize,

    /// How much data is going to be refilled by the caller?
    ///
    /// If there is no need to refill, then `refill_start` returns 0 and never sets this value, thus NonZero.
    pending_refill: Option<NonZeroUsize>,

    /// How much data will be consumed?
    ///
    /// Unlike pending refills, this can be 0.  We allow callers to ask for 0 items just for simplicity's sake.
    pending_consume: Option<usize>,
}

impl<Buffer: RefillableWrapped> RefillableWrapper<Buffer> {
    pub(crate) fn new(buffer: Buffer) -> Self {
        Self {
            buffer,
            start: 0,
            end: 0,
            pending_refill: None,
            pending_consume: None,
        }
    }

    /// Return how many items may be fulfilled at most without first adding more data.
    pub(crate) fn available(&self) -> usize {
        self.end - self.start
    }

    /// Return a slice which must be filled, attempting to offer the ability to refill `wanted` or more items.
    ///
    /// Returns `None`  when the buffer is completely full.  When there is space in the buffer, returns a slice
    /// representing that space, of possibly more than `wanted` items (it is safe to call `refill_end` with less).  If
    /// some value `x` has just been consumed and that value is passed to this function, the slice will be at least `x`
    /// items long (so, for example, producing and consuming fixed blocks with a buffer which is a multiple of the block
    /// size works out).
    ///
    /// The returned slice is not zeroed, and so shouldn't be read or added to, only written.
    ///
    /// # Panics
    ///
    /// If the specified length can never be fulfilled because it is longer than the length of the wrapped buffer, we
    /// panic; this is a logic error.
    pub(crate) fn refill_start(&mut self, wanted: usize) -> Option<Buffer::SlicedMut<'_>> {
        if self.pending_consume.is_some() {
            panic!("It is not possible to refill this buffer while consuming from it");
        }

        assert!(
            wanted <= self.buffer.len(),
            "Requests longer than the buffer can never be fulfilled"
        );

        if self.available() == self.buffer.len() {
            return None;
        }

        // Before doing anything, rotate to the front if `wanted` can't be filled in one call.
        if self.buffer.len() - self.end < wanted {
            self.buffer.copy_to_beginning(self.start..self.end);
            self.end -= self.start;
            self.start = 0;
        } else if self.start == self.end {
            // Maybe the buffer is empty. If so, it's a waste to not just write from the beginning.
            self.start = 0;
            self.end = 0;
        }

        let len = self.buffer.len() - self.end;
        self.pending_refill = Some(
            NonZeroUsize::new(len).expect("If this range is empty, we should have gotten a None"),
        );
        Some(self.buffer.slice_mut(self.end..self.buffer.len()))
    }

    /// This convenience method starts a refill which will refill the whole buffer.
    ///
    /// It is equivalent to `refill_start(total_length)`.
    pub(crate) fn refill_start_all(&mut self) -> Option<Buffer::SlicedMut<'_>> {
        self.refill_start(self.buffer.len())
    }

    /// Finish a refill.
    ///
    /// This should only be called if `refill_start` returns `Some(slice)`, the slice should be filled at least for the
    /// first `did` items, and `did <= slice.len()`.
    ///
    /// The next consumption will return at least `did` items.
    ///
    /// # panics
    ///
    /// If `did` is too big.
    pub(crate) fn refill_end(&mut self, did: usize) {
        let pending = self
            .pending_refill
            .expect("No refill was in progress")
            .get();
        assert!(
            did <= pending,
            "This refill completion does not match the call to refill_start"
        );
        self.end += did;
        self.pending_refill = None;
        debug_assert!(self.end <= self.buffer.len());
    }

    ///  Return all the data in the buffer.
    ///
    /// `consume_end` must afterwords be called with a value no more than the length of the returned slice.
    ///
    /// The returned slice is all valid data currently in the buffer, and so may be much larger than the last refill.
    ///
    /// # Panics
    ///
    /// Panics if a refill operation is in progress.
    fn consume_start(&mut self) -> Buffer::Sliced<'_> {
        if self.pending_refill.is_some() {
            panic!("It is not possible to consume while refilling");
        }

        self.pending_consume = Some(self.available());
        self.buffer.slice(self.start..self.end)
    }

    pub(crate) fn consume_end(&mut self, consumed: usize) {
        let last = self
            .pending_consume
            .expect("No consume operation was in progress");
        assert!(
            consumed <= last,
            "consume_end's got should be <= what was returned by the last consume_start"
        );
        self.start += consumed;
        debug_assert!(self.start <= self.end);
        self.pending_consume = None;
    }

    #[track_caller]
    fn assert_no_refill_consume(&self) {
        if self.pending_refill.is_some() {
            panic!("A refill operation is in progress");
        }

        if self.pending_consume.is_some() {
            panic!("A consume operation is in progress");
        }
    }

    /// Access the underlying buffer.
    ///
    /// This function cannot be called while a refill or consume is in progress.
    ///
    /// # Panics
    ///
    /// Panics if a refill or consume is in progress.
    pub(crate) fn get_buffer(&self) -> &Buffer {
        self.assert_no_refill_consume();
        &self.buffer
    }

    /// Access the underlying buffer mutably.
    ///
    /// This function cannot be called if a refill or consume is in progress.
    ///
    /// # Panics
    ///
    /// Panics if a refill or consume is in progress.
    pub(crate) fn get_buffer_mut(&mut self) -> &mut Buffer {
        self.assert_no_refill_consume();
        &mut self.buffer
    }

    /// Reset this buffer so that it contains no data and no operations are in progress.
    ///
    /// The underlying storage is not zeroed or anything along those lines.
    pub(crate) fn reset(&mut self) {
        self.start = 0;
        self.end = 0;
        self.pending_consume = None;
        self.pending_refill = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All of our tests do the same thing: write to the buffer and then consume, using a set of buffer sizes.
    ///
    /// We can therefore do all tests as one function, repeatedly called.
    ///
    /// Specifically, the buffer is filled with consecutive integers, and we expect to see those out on the far side.
    struct TestOpts {
        /// Size of the buffer.
        bufsize: usize,

        /// How high do we need to go before stopping?
        max: u64,

        /// We cycle through this array, calling refill_start with these values.
        ///
        /// `None` means fill the whole buffer.
        refill_sizes: &'static [Option<usize>],

        /// When consuming, we consume this much data each time.
        ///
        /// `None` means consume the whole buffer.
        consume_sizes: &'static [Option<usize>],
    }

    fn do_test(opts: TestOpts) {
        let backing = vec![0u64; opts.bufsize];
        let mut buffer = RefillableWrapper::new(backing);

        let refill_sizes = std::iter::repeat(opts.refill_sizes)
            .flat_map(|x| x.iter())
            .cloned();
        let consume_sizes = std::iter::repeat(opts.consume_sizes)
            .flat_map(|x| x.iter())
            .cloned();

        let mut highest_refilled = 0u64;
        let mut highest_consumed = 0u64;

        // This is an infinite loop, so `.take` with an unreasonably high value and panic if it ever gets there.
        for (refill_size, consume_size) in refill_sizes.zip(consume_sizes).take(100000) {
            let refill_size = refill_size.unwrap_or(buffer.get_buffer().len());
            let consume_size = consume_size.unwrap_or(buffer.get_buffer().len());

            {
                let rslice = buffer.refill_start(refill_size);
                if let Some(rslice) = rslice {
                    let rslice_len = rslice.len();
                    for i in rslice.iter_mut() {
                        *i = highest_refilled;
                        highest_refilled += 1;
                    }

                    buffer.refill_end(rslice_len);
                }
            }

            let cslice = buffer.consume_start();
            let cslice_len = cslice.len();
            for i in cslice.iter().copied().take(consume_size) {
                assert_eq!(i, highest_consumed);
                highest_consumed += 1;
            }

            buffer.consume_end(cslice_len.min(consume_size));

            if highest_consumed >= opts.max {
                // not break; break panics because the test thinks it failed to make progress.
                return;
            }
        }

        panic!("If the test gets here, then that means we failed to make progress");
    }

    #[test]
    fn test_boring() {
        do_test(TestOpts {
            bufsize: 100,
            max: 1000,
            consume_sizes: &[Some(1)],
            refill_sizes: &[Some(1)],
        });
    }

    #[test]
    fn test_complicated() {
        do_test(TestOpts {
            bufsize: 100,
            max: 100000,
            consume_sizes: &[Some(1), Some(3), Some(5), None],
            refill_sizes: &[Some(1), Some(15), Some(27), None, Some(34), None],
        });
    }
}

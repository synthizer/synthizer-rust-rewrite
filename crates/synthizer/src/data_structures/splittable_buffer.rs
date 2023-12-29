use std::num::NonZeroUsize;

use crate::config::*;
/// Type representing some slices split from [SplittableBuffer]
///
/// generic over the slice type, so this can work for both shared and mutable; `Slice` is `&[T]` or `&mut [T]`.
type SplitSlices<Slice> = arrayvec::ArrayVec<Slice, MAX_CHANNELS>;

/// A heap-allocated buffer which may be split into up to `MAX_CHANNELS` subslices.
///
/// This is useful because Rust makes splitting slices up tricky especially in a realtime context.  This buffer is
/// always a multiple of the specified channel count at creation in length and calling `.split()` returns exactly
/// channels subslices all of the same length.
///
/// It may be indexed as if it were a vec as well, including slicing.  This enables using the buffer contiguously, e.g.
/// for already interleaved audio data.
///
/// calling anything but `reserve` and `push` is realtime-safe, as long as mutating `T` is realtime-safe.  Dropping the
/// buffer is *not* realtime-safe; if this type must exist and drop on the audio thread, it must be deferred via
/// [crate::background_drop] either directly or (as is the usual case) indirectly as part of something else being
/// dropped.  If this is part of a node, this is handled; deregistration of nodes already defers dropping of the entire
/// node.
pub(crate) struct SplittableBuffer<T> {
    buffer: Vec<T>,
    channels: NonZeroUsize,
}

impl<T> SplittableBuffer<T> {
    /// Create an empty buffer which has not allocated yet.
    ///
    /// # panics
    ///
    /// Panics if the input channel counter is more than `MAX_CHANNELS`.  Since this is internal, we assume that's a
    /// logic error.
    pub(crate) fn new(channels: NonZeroUsize) -> Self {
        assert!(channels.get() <= MAX_CHANNELS);

        Self {
            buffer: vec![],
            channels,
        }
    }

    pub(crate) fn with_capacity(channels: NonZeroUsize, capacity_in_frames: usize) -> Self {
        let mut ret = Self::new(channels);
        ret.reserve(capacity_in_frames);
        ret
    }

    /// Reserve space for up to `frames` in the buffer.
    ///
    /// That is, reserved space is `frames * channels`.
    pub(crate) fn reserve(&mut self, frames: usize) {
        self.buffer.reserve(frames * self.channels.get());
    }

    /// Fill the buffer with the specified item, up to the specified number of frames.
    ///
    /// Existing data is untouched.
    fn extend(&mut self, total_frames: usize, item: T)
    where
        T: Clone,
    {
        self.reserve(total_frames);
        self.buffer
            .resize_with(total_frames * self.channels.get(), || item.clone());
    }
    /// Push an item to all channels.
    pub(crate) fn push_broadcast(&mut self, item: T)
    where
        T: Clone,
    {
        for _ in 0..self.channels.get() {
            self.buffer.push(item.clone());
        }
    }

    /// Push a frame of data.  The frame is an array of any size, but must match the channel count specified at buffer
    /// creation.  Allows giving direct ownership of items.
    ///
    /// # Panics
    ///
    /// If `N` isn't exactly one frame of data, panics.
    pub fn push_array<const N: usize>(&mut self, items: [T; N]) {
        assert_eq!(N, self.channels.get());
        for x in items.into_iter() {
            self.buffer.push(x);
        }
    }

    /// Push a frame of default data.
    pub fn push_default_frame(&mut self)
    where
        T: Default,
    {
        for _ in 0..self.channels.get() {
            self.buffer.push(Default::default())
        }
    }

    /// Get the length of this buffer in frames.
    pub(crate) fn len_frames(&self) -> usize {
        self.buffer.len() / self.channels.get()
    }

    /// Get the length of this buffer in items.
    pub(crate) fn len_items(&self) -> usize {
        self.buffer.len()
    }

    /// Split this buffer into `channels` subslices.
    pub(crate) fn split(&self) -> SplitSlices<&[T]> {
        let mut out = SplitSlices::<&[T]>::new();

        // This is the simpler case, where we can just directly push and return. Mutable slices are harder.
        let frames = self.len_frames();

        for i in 0..self.channels.get() {
            out.push(&self.buffer[i * frames..(i + 1) * frames]);
        }

        out
    }

    /// Split this buffer into `channels` mutable slices.
    pub fn split_mut(&mut self) -> SplitSlices<&mut [T]> {
        let mut out = SplitSlices::<&mut [T]>::new();

        // The way this works is as follows.  Rust is happy to give us one giant slice and to also let us split that
        // slice in two, but will not allow us to mutably slice multiple times.  To make this work, slice once and then
        // repeatedly split off the front.
        let frames = self.len_frames();
        let mut remaining = &mut self.buffer[..];

        for _ in 0..self.channels.get() {
            let (front, rest) = remaining.split_at_mut(frames);
            remaining = rest;
            out.push(front);
        }

        assert!(remaining.is_empty());
        out
    }
}

impl<T, I> std::ops::Index<I> for SplittableBuffer<T>
where
    Vec<T>: std::ops::Index<I>,
{
    type Output = <Vec<T> as std::ops::Index<I>>::Output;

    fn index(&self, index: I) -> &Self::Output {
        self.buffer.index(index)
    }
}

impl<T, I> std::ops::IndexMut<I> for SplittableBuffer<T>
where
    Vec<T>: std::ops::IndexMut<I>,
{
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        self.buffer.index_mut(index)
    }
}

impl<T: Copy + 'static> super::RefillableWrapped for SplittableBuffer<T> {
    type Sliced<'a> = SplitSlices<&'a [T]>;
    type SlicedMut<'a> = SplitSlices<&'a mut [T]>;

    fn len(&self) -> usize {
        self.len_frames()
    }

    fn slice(&self, range: std::ops::Range<usize>) -> Self::Sliced<'_> {
        self.split()
            .into_iter()
            .map(|x| &x[range.clone()])
            .collect()
    }

    fn slice_mut(&mut self, range: std::ops::Range<usize>) -> Self::SlicedMut<'_> {
        self.split_mut()
            .into_iter()
            .map(|x| &mut x[range.clone()])
            .collect()
    }

    fn copy_to_beginning(&mut self, range: std::ops::Range<usize>) {
        for s in self.split_mut().into_iter() {
            s.copy_within(range.clone(), 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Produce a vec [1,2,3,max-1].
    fn seq(min: u64, max: u64) -> Vec<u64> {
        assert!(max >= min);
        (min..max).collect::<Vec<_>>()
    }

    /// Create a SplittableBuffer filled as 0, 1, 2, 3, ... max - 1.
    fn splitbuf(chans: usize, max: u64) -> SplittableBuffer<u64> {
        let mut ret =
            SplittableBuffer::with_capacity(NonZeroUsize::new(chans).unwrap(), max as usize);
        // We don't really have a way to push one item at a time by design, but that's the easiest way to build this
        // mock data.
        assert_eq!(max % chans as u64, 0);
        for i in 0..max {
            ret.buffer.push(i);
        }

        ret
    }

    #[test]
    fn test_basic_splitting() {
        let buf = splitbuf(3, 9);
        let split = buf.split();

        assert_eq!(split.len(), 3);
        assert_eq!(split[0], &seq(0, 3));
        assert_eq!(split[1], &seq(3, 6));
        assert_eq!(split[2], &seq(6, 9));
    }

    // This test exists because the split_mut implementation is more complicated; the code is identical to the above
    // test except that split becomes split_mut.
    #[test]
    fn test_basic_splitting_mut() {
        let mut buf = splitbuf(3, 9);
        let split = buf.split_mut();

        assert_eq!(split.len(), 3);
        assert_eq!(split[0], &seq(0, 3));
        assert_eq!(split[1], &seq(3, 6));
        assert_eq!(split[2], &seq(6, 9));
    }

    #[test]
    fn test_push_default() {
        let mut buf = SplittableBuffer::<u64>::new(NonZeroUsize::new(3).unwrap());
        buf.push_default_frame();
        buf.push_default_frame();
        assert_eq!(&buf.buffer, &vec![0; 6]);
    }

    #[test]
    fn test_push_array() {
        let mut buf = SplittableBuffer::<u64>::new(NonZeroUsize::new(3).unwrap());
        buf.push_array([0, 1, 2]);
        buf.push_array([3, 4, 5]);
        assert_eq!(&buf.buffer, &seq(0, 6));
    }

    #[test]
    #[should_panic]
    fn test_push_array_length_mismatch() {
        let mut buf = SplittableBuffer::<u64>::new(NonZeroUsize::new(3).unwrap());
        buf.push_array([0, 1]);
    }

    #[test]
    fn test_push_broadcast() {
        let mut buf = SplittableBuffer::<u64>::new(NonZeroUsize::new(3).unwrap());
        buf.push_broadcast(0);
        buf.push_broadcast(1);
        assert_eq!(buf.buffer, vec![0, 0, 0, 1, 1, 1]);
    }
}

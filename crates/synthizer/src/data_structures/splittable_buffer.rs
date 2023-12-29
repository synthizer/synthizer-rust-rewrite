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
/// This is a realtime safe type, save for methods which grow. Dropping it is *not* realtime-safe; if this type must
/// exist and drop on the audio thread, it must be deferred via [crate::background_drop] either directly or (as is the
/// usual case) indirectly as part of something else being dropped.  If this is part of a node, this is handled;
/// deregistration of nodes already defers dropping of the entire node.
pub(crate) struct SplittableBuffer<T> {
    buffer: Vec<T>,
    channels: NonZeroUsize,
}

impl<T> SplittableBuffer<T> {
    pub(crate) fn new(channels: NonZeroUsize, capacity_in_frames: usize) -> Self
    where
        T: Default + Clone,
    {
        Self::new_with_default(channels, capacity_in_frames, Default::default())
    }

    pub(crate) fn new_with_default(
        channels: NonZeroUsize,
        capacity_in_frames: usize,
        default_val: T,
    ) -> Self
    where
        T: Clone,
    {
        Self::new_with(channels, capacity_in_frames, |_, _| default_val.clone())
    }

    /// Create a buffer using a closure which takes the channel and index of the item to insert as `(channel, offset)`.
    ///
    /// This closure will be called in the order items are inserted, which means that `channel` varies slowest.
    pub(crate) fn new_with(
        channels: NonZeroUsize,
        capacity_in_frames: usize,
        mut closure: impl FnMut(usize, usize) -> T,
    ) -> Self {
        let mut ret = Self {
            channels,
            buffer: vec![],
        };

        let underlying_cap = channels.get() * capacity_in_frames;
        ret.buffer.reserve(underlying_cap);

        for c in 0..channels.get() {
            for i in 0..capacity_in_frames {
                ret.buffer.push(closure(c, i));
            }
        }

        ret
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
    fn splitbuf(chans: usize, max: usize) -> SplittableBuffer<u64> {
        let frames = max / chans;
        assert_eq!(chans * frames, max);

        SplittableBuffer::new_with(NonZeroUsize::new(chans).unwrap(), frames, |c, f| {
            (c * frames + f) as u64
        })
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
}

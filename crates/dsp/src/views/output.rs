#![allow(clippy::needless_range_loop)] // this is clearer, and possibly faster.
use crate::ViewMeta;

/// A view over somewhere where audio can be written to.
///
/// This is like a slice, but it is not possible to read the slice.  The difference is that sometimes it is backed by a
/// circular buffer, sometimes it is adding instead of just writing, etc.
///
/// OutputViews also hold a number of channels, but all operations are done with raw indices.  This is so that we can
/// move the proofs of bounds checks closer to the users, and hopefully get LLVM to see through things without regularly
/// using unsafe.
pub trait OutputView: ViewMeta {
    /// Write a sample of audio data.
    fn write_index(&mut self, index: usize, sample: Self::SampleType);

    /// Write a sample of audio data, with unchecked bounds.
    ///
    /// # Safety
    ///
    /// If the index is out of bounds, behavior is undefined.
    #[inline(always)]
    unsafe fn write_index_unchecked(&mut self, index: usize, sample: Self::SampleType) {
        self.write_index(index, sample);
    }
}

/// An output buffer which is a vew over a slice.
///
/// if `the const generic ADD` is true, then the output will have values added to the slice rather than written;
/// otherwise, they're just written.
pub struct SliceOutputView<'a, T, const ADD: bool> {
    backing_slice: &'a mut [T],
    channels: usize,
}

impl<'a, T, const ADD: bool> SliceOutputView<'a, T, ADD> {
    #[inline(always)]
    pub fn new(slice: &'a mut [T], channels: usize) -> Self {
        assert_eq!(slice.len() % channels, 0);
        Self {
            channels,
            backing_slice: slice,
        }
    }
}

impl<'a, T, const ADD: bool> ViewMeta for SliceOutputView<'a, T, ADD>
where
    T: std::ops::AddAssign + Copy,
{
    type SampleType = T;

    #[inline(always)]
    fn get_channels(&self) -> usize {
        self.channels
    }

    #[inline(always)]
    fn get_frames(&self) -> usize {
        self.backing_slice.len() / self.channels
    }

    #[inline(always)]
    fn get_len(&self) -> usize {
        self.backing_slice.len()
    }
}

impl<'a, T, const ADD: bool> OutputView for SliceOutputView<'a, T, ADD>
where
    T: std::ops::AddAssign + Copy,
{
    #[inline(always)]
    fn write_index(&mut self, index: usize, sample: Self::SampleType) {
        if ADD {
            self.backing_slice[index] += sample;
        } else {
            self.backing_slice[index] = sample;
        }
    }

    #[inline(always)]
    unsafe fn write_index_unchecked(&mut self, index: usize, sample: Self::SampleType) {
        if ADD {
            unsafe { *self.backing_slice.get_unchecked_mut(index) += sample };
        } else {
            *self.backing_slice.get_unchecked_mut(index) = sample;
        }
    }
}

/// An output buffer which runs over an array of statically known size, but which has a dynamic channel count.
///
/// These are useful because the array length being statically known serves as a type-level proof that the array is of
/// the right length, even if the array is bigger than what is needed.
///
/// If `ADD` is true, the array is added to, otherwise the array is set.
pub struct DynamicChannelsArrayOutputView<'a, T, const LEN: usize, const ADD: bool> {
    backing_array: &'a mut [T; LEN],
    channels: usize,
}

impl<'a, T, const LEN: usize, const ADD: bool> DynamicChannelsArrayOutputView<'a, T, LEN, ADD> {
    #[inline(always)]
    pub fn new(backing_array: &'a mut [T; LEN], channels: usize) -> Self {
        assert_eq!(LEN % channels, 0);

        Self {
            backing_array,
            channels,
        }
    }
}

impl<'a, T, const LEN: usize, const ADD: bool> ViewMeta
    for DynamicChannelsArrayOutputView<'a, T, LEN, ADD>
where
    T: Copy + std::ops::AddAssign,
{
    type SampleType = T;

    #[inline(always)]
    fn get_channels(&self) -> usize {
        self.channels
    }

    #[inline(always)]
    fn get_frames(&self) -> usize {
        LEN / self.channels
    }

    #[inline(always)]
    fn get_len(&self) -> usize {
        LEN
    }
}

impl<'a, T, const LEN: usize, const ADD: bool> OutputView
    for DynamicChannelsArrayOutputView<'a, T, LEN, ADD>
where
    T: Copy + std::ops::AddAssign,
{
    #[inline(always)]
    fn write_index(&mut self, index: usize, sample: Self::SampleType) {
        if ADD {
            self.backing_array[index] += sample;
        } else {
            self.backing_array[index] = sample;
        }
    }

    #[inline(always)]
    unsafe fn write_index_unchecked(&mut self, index: usize, sample: Self::SampleType) {
        if ADD {
            unsafe { *self.backing_array.get_unchecked_mut(index) += sample };
        } else {
            unsafe { *self.backing_array.get_unchecked_mut(index) = sample };
        }
    }
}

/// An output buffer which runs over an array of statically known size representing data of statically known channel
/// count.
///
/// These are useful because the array length being statically known serves as a type-level proof that the array is of
/// the right length, even if the array is bigger than what is needed.  This case also adds compile-time knowledge of
/// the channel count.
///
/// If `ADD` is true, the array is added to, otherwise the array is set.
pub struct StaticChannelsArrayOutputView<
    'a,
    T,
    const LEN: usize,
    const CHANS: usize,
    const ADD: bool,
> {
    backing_array: &'a mut [T; LEN],
}

impl<'a, T, const LEN: usize, const CHANS: usize, const ADD: bool>
    StaticChannelsArrayOutputView<'a, T, LEN, CHANS, ADD>
{
    #[inline(always)]
    pub fn new(backing_array: &'a mut [T; LEN]) -> Self {
        assert_eq!(LEN % CHANS, 0);
        Self { backing_array }
    }
}

impl<'a, T, const LEN: usize, const CHANS: usize, const ADD: bool> ViewMeta
    for StaticChannelsArrayOutputView<'a, T, LEN, CHANS, ADD>
where
    T: Copy + std::ops::AddAssign,
{
    type SampleType = T;

    #[inline(always)]
    fn get_channels(&self) -> usize {
        CHANS
    }

    #[inline(always)]
    fn get_frames(&self) -> usize {
        LEN / CHANS
    }

    #[inline(always)]
    fn get_len(&self) -> usize {
        LEN
    }
}

impl<'a, T, const LEN: usize, const CHANS: usize, const ADD: bool> OutputView
    for StaticChannelsArrayOutputView<'a, T, LEN, CHANS, ADD>
where
    T: Copy + std::ops::AddAssign,
{
    #[inline(always)]
    fn write_index(&mut self, index: usize, sample: Self::SampleType) {
        if ADD {
            self.backing_array[index] += sample;
        } else {
            self.backing_array[index] = sample;
        }
    }

    #[inline(always)]
    unsafe fn write_index_unchecked(&mut self, index: usize, sample: Self::SampleType) {
        if ADD {
            unsafe { *self.backing_array.get_unchecked_mut(index) += sample };
        } else {
            unsafe {
                *self.backing_array.get_unchecked_mut(index) = sample;
            }
        }
    }
}

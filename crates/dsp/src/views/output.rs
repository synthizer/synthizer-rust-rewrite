#![allow(clippy::needless_range_loop)] // this is clearer, and possibly faster.
use super::structs::*;
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

impl<'a, T, const ADD: bool> OutputView for SliceView<'a, T, ADD>
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

impl<'a, T, const LEN: usize, const ADD: bool> OutputView
    for DynamicChannelsArrayView<'a, T, LEN, ADD>
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

impl<'a, T, const LEN: usize, const CHANS: usize, const ADD: bool> OutputView
    for StaticChannelsArrayView<'a, T, LEN, CHANS, ADD>
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

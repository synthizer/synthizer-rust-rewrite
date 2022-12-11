use super::iter::*;
use super::*;

/// An input view is like a slice, but backed by any number of things.
///
/// All input views have channel counts, but expose only the raw index in order to aid in getting bounds checking etc. closer to where they're used.
pub trait InputView: ViewMeta {
    fn read_index(&self, index: usize) -> Self::SampleType;

    /// Read a sample without checking indices.
    ///
    /// # Safety
    ///
    /// If the index is out of bounds, behavior is undefined.
    unsafe fn read_sample_unchecked(&self, index: usize) -> Self::SampleType {
        self.read_index(index)
    }

    fn iter(&self) -> ViewIter<'_, Self>
    where
        Self: Sized,
    {
        ViewIter {
            view: self,
            index: 0,
        }
    }
}

impl<'a, T> InputView for ImmutableSliceView<'a, T>
where
    T: Copy,
{
    #[inline(always)]
    fn read_index(&self, index: usize) -> Self::SampleType {
        self.backing_slice[index]
    }

    unsafe fn read_sample_unchecked(&self, index: usize) -> Self::SampleType {
        *self.backing_slice.get_unchecked(index)
    }
}

impl<'a, T, const ADD: bool> InputView for MutableSliceView<'a, T, ADD>
where
    T: Copy,
{
    #[inline(always)]
    fn read_index(&self, index: usize) -> Self::SampleType {
        self.backing_slice[index]
    }

    unsafe fn read_sample_unchecked(&self, index: usize) -> Self::SampleType {
        *self.backing_slice.get_unchecked(index)
    }
}

impl<'a, T, const LEN: usize> InputView for ImmutableDynamicChannelsArrayView<'a, T, LEN>
where
    T: Copy,
{
    #[inline(always)]
    fn read_index(&self, index: usize) -> Self::SampleType {
        self.backing_array[index]
    }

    #[inline(always)]
    unsafe fn read_sample_unchecked(&self, index: usize) -> Self::SampleType {
        *self.backing_array.get_unchecked(index)
    }
}

impl<'a, T, const LEN: usize, const ADD: bool> InputView
    for MutableDynamicChannelsArrayView<'a, T, LEN, ADD>
where
    T: Copy,
{
    #[inline(always)]
    fn read_index(&self, index: usize) -> Self::SampleType {
        self.backing_array[index]
    }

    #[inline(always)]
    unsafe fn read_sample_unchecked(&self, index: usize) -> Self::SampleType {
        *self.backing_array.get_unchecked(index)
    }
}

impl<'a, T, const LEN: usize, const CHANS: usize> InputView
    for ImmutableStaticChannelsArrayView<'a, T, LEN, CHANS>
where
    T: Copy,
{
    #[inline(always)]
    fn read_index(&self, index: usize) -> Self::SampleType {
        self.backing_array[index]
    }

    #[inline(always)]
    unsafe fn read_sample_unchecked(&self, index: usize) -> Self::SampleType {
        *self.backing_array.get_unchecked(index)
    }
}

impl<'a, T, const LEN: usize, const CHANS: usize, const ADD: bool> InputView
    for MutableStaticChannelsArrayView<'a, T, LEN, CHANS, ADD>
where
    T: Copy,
{
    #[inline(always)]
    fn read_index(&self, index: usize) -> Self::SampleType {
        self.backing_array[index]
    }

    #[inline(always)]
    unsafe fn read_sample_unchecked(&self, index: usize) -> Self::SampleType {
        *self.backing_array.get_unchecked(index)
    }
}

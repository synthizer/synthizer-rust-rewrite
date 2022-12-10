use super::*;

/// metadata for a view.
///
/// All views must expose their tagged channel count and the number of frames they have, as well as the underlying
/// length of the backing data (always `frames * channels`, but sometimes more "obvious" to the compiler).
pub trait ViewMeta {
    /// The kind of data this view holds.
    type SampleType: Copy;

    /// Get the number of frames in this view.
    fn get_frames(&self) -> usize;

    /// Get the number of channels in this view.
    fn get_channels(&self) -> usize;

    /// Get the length of the raw data.
    ///
    /// Always `frames * channels` (contractually; an implementation which doesn't do that is invalid), but sometimes
    /// more "obvious" to the compiler, or via a cached value, or etc. to make evaluating that expression as cheap as
    /// possible.
    fn get_len(&self) -> usize;
}

impl<'a, T, const ADD: bool> ViewMeta for MutableSliceView<'a, T, ADD>
where
    T: Copy,
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

impl<'a, T> ViewMeta for ImmutableSliceView<'a, T>
where
    T: Copy,
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

impl<'a, T, const LEN: usize, const CHANS: usize, const ADD: bool> ViewMeta
    for MutableStaticChannelsArrayView<'a, T, LEN, CHANS, ADD>
where
    T: Copy,
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

impl<'a, T, const LEN: usize, const CHANS: usize> ViewMeta
    for ImmutableStaticChannelsArrayView<'a, T, LEN, CHANS>
where
    T: Copy,
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

impl<'a, T, const LEN: usize, const ADD: bool> ViewMeta
    for MutableDynamicChannelsArrayView<'a, T, LEN, ADD>
where
    T: Copy,
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

impl<'a, T, const LEN: usize> ViewMeta for ImmutableDynamicChannelsArrayView<'a, T, LEN>
where
    T: Copy,
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

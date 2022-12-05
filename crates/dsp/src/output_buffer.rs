#![allow(clippy::needless_range_loop)] // this is clearer, and possibly faster.

/// A destination for audio.
///
/// This abstracts over a few possibilities: the kind of the output buffer (e.g. a slice, a an array, etc) and whether
/// or not the DSP algorithm is writing to or adding to an output.
///
/// Output buffers don't maintain an index.  That's up to the caller.
pub trait OutputBuffer {
    type SampleType;

    /// get the total number of frames which may be written.
    fn get_frames(&self) -> usize;

    /// Get the channels in a frame.
    fn get_channels(&self) -> usize;

    /// Write a sample of audio data.
    fn write_sample(&mut self, frame: usize, channel: usize, sample: Self::SampleType);

    /// Write a sample of audio data, with unchecked bounds.
    ///
    /// # Safety
    ///
    /// If the indices are out of bounds, behavior is undefined.
    unsafe fn write_sample_unchecked(
        &mut self,
        frame: usize,
        channel: usize,
        sample: Self::SampleType,
    ) {
        self.write_sample(frame, channel, sample);
    }
}

/// An output buffer which is a vew over a slice.
///
/// if `ADD` is true, then the output will have values added to the slice rather than written; otherwise, they're just
/// written.
pub struct SliceOutputBuffer<'a, T, const ADD: bool> {
    backing_slice: &'a mut [T],
    channels: usize,
    frame_count: usize,
}

impl<'a, T, const ADD: bool> SliceOutputBuffer<'a, T, ADD> {
    pub fn new(slice: &'a mut [T], channels: usize) -> Self {
        Self {
            channels,
            frame_count: slice.len() / channels,
            backing_slice: slice,
        }
    }
}

impl<'a, T, const ADD: bool> OutputBuffer for SliceOutputBuffer<'a, T, ADD>
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
        self.frame_count
    }

    #[inline(always)]
    fn write_sample(&mut self, frame: usize, channel: usize, sample: Self::SampleType) {
        let ind = frame * self.channels + channel;
        if ADD {
            self.backing_slice[ind] += sample;
        } else {
            self.backing_slice[ind] = sample;
        }
    }

    #[inline(always)]
    unsafe fn write_sample_unchecked(
        &mut self,
        frame: usize,
        channel: usize,
        sample: Self::SampleType,
    ) {
        let ind = self.channels * frame + channel;
        if ADD {
            unsafe { *self.backing_slice.get_unchecked_mut(ind) += sample };
        } else {
            *self.backing_slice.get_unchecked_mut(ind) = sample;
        }
    }
}

/// An output buffer which runs over an array of statically known size, but which has a dynamic channel count.
///
/// These are useful because the array length being statically known serves as a type-level proof that the array is of
/// the right length, even if the array is bigger than what is needed.
///
/// If `ADD` is true, the array is added to, otherwise the array is set.
pub struct DynamicChannelsArrayOutputBuffer<'a, T, const LEN: usize, const ADD: bool> {
    backing_array: &'a mut [T; LEN],
    channels: usize,
}

impl<'a, T, const LEN: usize, const ADD: bool> DynamicChannelsArrayOutputBuffer<'a, T, LEN, ADD> {
    pub fn new(backing_array: &'a mut [T; LEN], channels: usize) -> Self {
        Self {
            backing_array,
            channels,
        }
    }
}

impl<'a, T, const LEN: usize, const ADD: bool> OutputBuffer
    for DynamicChannelsArrayOutputBuffer<'a, T, LEN, ADD>
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

    fn write_sample(&mut self, frame: usize, channel: usize, sample: Self::SampleType) {
        let ind = self.channels * frame + channel;
        if ADD {
            self.backing_array[ind] += sample;
        } else {
            self.backing_array[ind] = sample;
        }
    }

    unsafe fn write_sample_unchecked(
        &mut self,
        frame: usize,
        channel: usize,
        sample: Self::SampleType,
    ) {
        let ind = frame * self.channels + channel;
        if ADD {
            unsafe { *self.backing_array.get_unchecked_mut(ind) += sample };
        } else {
            unsafe { *self.backing_array.get_unchecked_mut(ind) = sample };
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
pub struct StaticChannelsArrayOutputBuffer<
    'a,
    T,
    const LEN: usize,
    const CHANS: usize,
    const ADD: bool,
> {
    backing_array: &'a mut [T; LEN],
}

impl<'a, T, const LEN: usize, const CHANS: usize, const ADD: bool>
    StaticChannelsArrayOutputBuffer<'a, T, LEN, CHANS, ADD>
{
    pub fn new(backing_array: &'a mut [T; LEN]) -> Self {
        Self { backing_array }
    }
}

impl<'a, T, const LEN: usize, const CHANS: usize, const ADD: bool> OutputBuffer
    for StaticChannelsArrayOutputBuffer<'a, T, LEN, CHANS, ADD>
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

    fn write_sample(&mut self, frame: usize, channel: usize, sample: Self::SampleType) {
        let ind = frame * CHANS + channel;
        if ADD {
            self.backing_array[ind] += sample;
        } else {
            self.backing_array[ind] = sample;
        }
    }

    unsafe fn write_sample_unchecked(
        &mut self,
        frame: usize,
        channel: usize,
        sample: Self::SampleType,
    ) {
        let ind = CHANS * frame + channel;
        if ADD {
            unsafe { *self.backing_array.get_unchecked_mut(ind) += sample };
        } else {
            unsafe {
                *self.backing_array.get_unchecked_mut(ind) = sample;
            }
        }
    }
}
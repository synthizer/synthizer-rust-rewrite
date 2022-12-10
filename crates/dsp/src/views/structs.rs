/// A buffer which is a vew over a slice.
///
/// if the const generic `ADD` is true and this is an output buffer, then the output will have values added to the slice rather than written;
/// otherwise, they're just written.
pub struct SliceView<'a, T, const ADD: bool> {
    pub(crate) backing_slice: &'a mut [T],
    pub(crate) channels: usize,
}

impl<'a, T, const ADD: bool> SliceView<'a, T, ADD> {
    #[inline(always)]
    pub fn new(slice: &'a mut [T], channels: usize) -> Self {
        assert_eq!(slice.len() % channels, 0);
        Self {
            channels,
            backing_slice: slice,
        }
    }
}

/// A buffer which runs over an array of statically known size, but which has a dynamic channel count.
///
/// These are useful because the array length being statically known serves as a type-level proof that the array is of
/// the right length, even if the array is bigger than what is needed.
///
/// If `ADD` is true and this is an output buffer, the array is added to, otherwise the array is set.
pub struct DynamicChannelsArrayView<'a, T, const LEN: usize, const ADD: bool> {
    pub(crate) backing_array: &'a mut [T; LEN],
    pub(crate) channels: usize,
}

impl<'a, T, const LEN: usize, const ADD: bool> DynamicChannelsArrayView<'a, T, LEN, ADD> {
    #[inline(always)]
    pub fn new(backing_array: &'a mut [T; LEN], channels: usize) -> Self {
        assert_eq!(LEN % channels, 0);

        Self {
            backing_array,
            channels,
        }
    }
}

/// A buffer which runs over an array of statically known size representing data of statically known channel
/// count.
///
/// These are useful because the array length being statically known serves as a type-level proof that the array is of
/// the right length, even if the array is bigger than what is needed.  This case also adds compile-time knowledge of
/// the channel count.
///
/// If `ADD` is true and this is an output buffer, the array is added to, otherwise the array is set.
pub struct StaticChannelsArrayView<'a, T, const LEN: usize, const CHANS: usize, const ADD: bool> {
    pub(crate) backing_array: &'a mut [T; LEN],
}

impl<'a, T, const LEN: usize, const CHANS: usize, const ADD: bool>
    StaticChannelsArrayView<'a, T, LEN, CHANS, ADD>
{
    #[inline(always)]
    pub fn new(backing_array: &'a mut [T; LEN]) -> Self {
        assert_eq!(LEN % CHANS, 0);
        Self { backing_array }
    }
}

pub struct MutableSliceView<'a, T, const ADD: bool> {
    pub(crate) backing_slice: &'a mut [T],
    pub(crate) channels: usize,
}

impl<'a, T, const ADD: bool> MutableSliceView<'a, T, ADD> {
    #[inline(always)]
    pub fn new(slice: &'a mut [T], channels: usize) -> Self {
        assert_eq!(slice.len() % channels, 0);
        Self {
            channels,
            backing_slice: slice,
        }
    }
}

pub struct ImmutableSliceView<'a, T> {
    pub(crate) backing_slice: &'a [T],
    pub(crate) channels: usize,
}

impl<'a, T> ImmutableSliceView<'a, T> {
    #[inline(always)]
    pub fn new(slice: &'a [T], channels: usize) -> Self {
        assert_eq!(slice.len() % channels, 0);
        Self {
            channels,
            backing_slice: slice,
        }
    }
}

pub struct MutableDynamicChannelsArrayView<'a, T, const LEN: usize, const ADD: bool> {
    pub(crate) backing_array: &'a mut [T; LEN],
    pub(crate) channels: usize,
}

impl<'a, T, const LEN: usize, const ADD: bool> MutableDynamicChannelsArrayView<'a, T, LEN, ADD> {
    #[inline(always)]
    pub fn new(backing_array: &'a mut [T; LEN], channels: usize) -> Self {
        assert_eq!(LEN % channels, 0);

        Self {
            backing_array,
            channels,
        }
    }
}

pub struct ImmutableDynamicChannelsArrayView<'a, T, const LEN: usize> {
    pub(crate) backing_array: &'a [T; LEN],
    pub(crate) channels: usize,
}

impl<'a, T, const LEN: usize> ImmutableDynamicChannelsArrayView<'a, T, LEN> {
    #[inline(always)]
    pub fn new(backing_array: &'a [T; LEN], channels: usize) -> Self {
        assert_eq!(LEN % channels, 0);

        Self {
            backing_array,
            channels,
        }
    }
}

pub struct MutableStaticChannelsArrayView<
    'a,
    T,
    const LEN: usize,
    const CHANS: usize,
    const ADD: bool,
> {
    pub(crate) backing_array: &'a mut [T; LEN],
}

impl<'a, T, const LEN: usize, const CHANS: usize, const ADD: bool>
    MutableStaticChannelsArrayView<'a, T, LEN, CHANS, ADD>
{
    #[inline(always)]
    pub fn new(backing_array: &'a mut [T; LEN]) -> Self {
        assert_eq!(LEN % CHANS, 0);
        Self { backing_array }
    }
}

pub struct ImmutableStaticChannelsArrayView<'a, T, const LEN: usize, const CHANS: usize> {
    pub(crate) backing_array: &'a [T; LEN],
}

impl<'a, T, const LEN: usize, const CHANS: usize>
    ImmutableStaticChannelsArrayView<'a, T, LEN, CHANS>
{
    #[inline(always)]
    pub fn new(backing_array: &'a [T; LEN]) -> Self {
        assert_eq!(LEN % CHANS, 0);
        Self { backing_array }
    }
}

pub type InputSliceView<'a, T> = ImmutableSliceView<'a, T>;
pub type OutputSliceView<'a, T, const ADD: bool> = MutableSliceView<'a, T, ADD>;
pub type BidirectionalSliceView<'a, T, const ADD: bool> = MutableSliceView<'a, T, ADD>;

pub type InputDynamicChannelsArrayView<'a, T, const LEN: usize> =
    ImmutableDynamicChannelsArrayView<'a, T, LEN>;
pub type OutputDynamicChannelsArrayView<'a, T, const LEN: usize, const ADD: bool> =
    MutableDynamicChannelsArrayView<'a, T, LEN, ADD>;
pub type BidirectionalDynamicChannelsArrayView<'a, T, const LEN: usize, const ADD: bool> =
    MutableDynamicChannelsArrayView<'a, T, LEN, ADD>;

pub type InputStaticChannelsArrayView<'a, T, const LEN: usize, const CHANS: usize> =
    ImmutableStaticChannelsArrayView<'a, T, LEN, CHANS>;
pub type OutputStaticChannelsArrayView<
    'a,
    T,
    const LEN: usize,
    const CHANS: usize,
    const ADD: bool,
> = MutableStaticChannelsArrayView<'a, T, LEN, CHANS, ADD>;
pub type BidirectionalStaticChannelsArrayView<
    'a,
    T,
    const LEN: usize,
    const CHANS: usize,
    const ADD: bool,
> = MutableStaticChannelsArrayView<'a, T, LEN, CHANS, ADD>;

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

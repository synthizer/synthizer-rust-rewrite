//! Implementation of audio frames.
//!
//! Mostly we need a static frame and a dynamic frame and we're done.

use crate::core_traits::*;

impl AudioFrame<f64> for f64 {
    fn channel_count(&self) -> usize {
        1
    }

    fn get(&self, index: usize) -> &f64 {
        debug_assert_eq!(index, 0);
        self
    }

    fn set(&mut self, index: usize, value: f64) {
        debug_assert_eq!(index, 0);
        *self = value;
    }
}

/// A wrapper over a frame which will return the default value of some `T` for indices outside the given range of the
/// underlying frame.
pub(crate) struct DefaultingFrameWrapper<'a, T, Inner>(&'a mut Inner, T);

impl<'a, T, Inner> DefaultingFrameWrapper<'a, T, Inner>
where
    T: Default,
{
    pub(crate) fn new(inner: &'a mut Inner) -> Self {
        DefaultingFrameWrapper(inner, Default::default())
    }

    /// Convert an array of frames to an array of wrappers.
    pub(crate) fn wrap_array<const N: usize>(array: &'a mut [Inner; N]) -> [Self; N] {
        crate::array_utils::collect_iter(
            array
                .iter_mut()
                .map(|x: &'a mut Inner| Self(x, Default::default())),
        )
    }
}

impl<T, Inner> AudioFrame<T> for DefaultingFrameWrapper<'_, T, Inner>
where
    Inner: AudioFrame<T>,
{
    fn channel_count(&self) -> usize {
        self.0.channel_count()
    }

    fn get(&self, index: usize) -> &T {
        if index >= self.0.channel_count() {
            return &self.1;
        }

        self.0.get(index)
    }

    fn set(&mut self, index: usize, value: T) {
        if index > self.0.channel_count() {
            return;
        }

        self.0.set(index, value);
    }
}

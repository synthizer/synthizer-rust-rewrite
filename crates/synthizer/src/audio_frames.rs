//! Implementation of audio frames.
//!
//! Mostly we need a static frame and a dynamic frame and we're done.
use crate::core_traits::*;

unsafe impl AudioFrame for f64 {
    fn channel_count(&self) -> usize {
        1
    }

    fn read_one<F: FnOnce(f64)>(&self, channel: usize, destination: F) {
        debug_assert_eq!(channel, 0);
        destination(*self);
    }

    fn read_all<F: FnMut(f64)>(&self, mut destination: F) {
        destination(*self)
    }
}

unsafe impl<const CH: usize> AudioFrame for [f64; CH] {
    fn channel_count(&self) -> usize {
        CH
    }

    fn read_one<F: FnOnce(f64)>(&self, channel: usize, destination: F) {
        destination(self[channel])
    }

    fn read_all<F: FnMut(f64)>(&self, mut destination: F) {
        for i in self.iter() {
            destination(*i)
        }
    }
}

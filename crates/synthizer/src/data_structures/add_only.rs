use crate::config::*;

use super::*;

/// A wrapper around [AllocatedBlock] which disables reading and forces the caller to only add to the output.
///
/// This is used with nodes, which must always add to their output.
pub(crate) struct AddOnlyBlock<'a>(&'a mut AllocatedBlock);

impl<'a> AddOnlyBlock<'a> {
    pub(crate) fn new(wrapping: &'a mut AllocatedBlock) -> Self {
        AddOnlyBlock(wrapping)
    }

    #[inline(always)]
    pub(crate) fn write(&mut self, index: usize, value: f64) {
        (self.0)[index] += value;
    }
}

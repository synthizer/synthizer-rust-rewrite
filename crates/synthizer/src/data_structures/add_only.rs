use crate::config::*;

use super::*;

/// A wrapper around [AllocatedBlock] which disables reading and forces the caller to only add to the output.
///
/// This is used with nodes, which must always add to their output.
pub(crate) struct AddOnlyBlock<'a>(&'a mut AllocatedBlock);

/// When dereferenced, an [AddOnlyBlock] becomes this type.
pub(crate) struct AddOnlyBlockDeref<'a>(&'a mut [f32; BLOCK_SIZE]);

impl<'a> AddOnlyBlock<'a> {
    pub(crate) fn new(wrapping: &'a mut AllocatedBlock) -> Self {
        AddOnlyBlock(wrapping)
    }

    pub(crate) fn deref_block(&mut self, allocator: &BlockAllocator) -> AddOnlyBlockDeref {
        AddOnlyBlockDeref(allocator.deref_block(self.0))
    }
}

impl<'a> AddOnlyBlockDeref<'a> {
    #[inline(always)]
    pub(crate) fn write(&mut self, index: usize, value: f32) {
        let s = &mut self.0;
        s[index] += value;
    }
}

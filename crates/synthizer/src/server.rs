use crate::data_structures::BlockAllocator;

/// Services a server may offer to a consumer on the audio thread.
pub(crate) struct AudioThreadServerServices {
    pub(crate) block_allocator: BlockAllocator,
}

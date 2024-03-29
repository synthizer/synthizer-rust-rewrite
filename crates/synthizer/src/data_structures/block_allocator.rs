use std::mem::MaybeUninit;

use audio_synchronization::concurrent_slab::{ExclusiveSlabRef, SlabHandle};

use crate::config::BlockArray;

/// Knows how to allocate blocks of f32 data for callers.
///
/// These are used to hold sampls.
pub struct BlockAllocator {
    slab: SlabHandle<MaybeUninit<crate::config::BlockArray>>,
}

/// An allocated block in a block allocator.
pub struct AllocatedBlock {
    handle: ExclusiveSlabRef<MaybeUninit<BlockArray>>,
}

impl BlockAllocator {
    pub fn new(capacity: usize) -> Self {
        BlockAllocator {
            slab: SlabHandle::new(capacity),
        }
    }

    /// Allocate a block, which is usually *not* zeroed.
    ///
    /// Zeroing is left to the caller because this is an expensive operation.  In debug builds, the returned buffer is guaranteed to contain random data.
    pub fn allocate_block(&self) -> AllocatedBlock {
        let handle = self.slab.insert(MaybeUninit::uninit());
        #[allow(unused_mut)] // it's used in debug builds.
        let mut ret = AllocatedBlock { handle };

        // We'll want to speed this up, but it's fine for now.
        #[cfg(debug_assertions)]
        {
            let mut rgen = crate::fast_xoroshiro::FastXoroshiro128PlusPlus::<1>::new_seeded(123);
            for o in (*ret).iter_mut() {
                let rval = rgen.gen_u64() as u16;
                *o = 1.0 - (rval as f64) / (u16::MAX as f64) * 2.0;
            }
        }

        ret
    }
}

// All bit patterns for arrays of f64 are valid, so the following two deref impls get to assume that the array is
// initialized.

impl std::ops::Deref for AllocatedBlock {
    type Target = BlockArray;

    fn deref(&self) -> &Self::Target {
        unsafe { self.handle.assume_init_ref() }
    }
}

impl std::ops::DerefMut for AllocatedBlock {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.handle.assume_init_mut() }
    }
}

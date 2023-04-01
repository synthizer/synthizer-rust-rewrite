use std::cell::UnsafeCell;
use std::rc::Rc;

use crate::config::BLOCK_SIZE;
use crate::unique_id::UniqueId;

/// Knows how to allocate blocks of f32 data for callers.
///
/// These are used to hold sampls.
pub struct BlockAllocator {
    inner: Rc<UnsafeCell<BlockAllocatorInner>>,
}

/// An allocated block in a block allocator.
///
/// When dropped, the blok is deallocated from the backing allocator.
///
/// Also serves as a type-level proof that this block is unique.
pub struct AllocatedBlock {
    allocator: Rc<UnsafeCell<BlockAllocatorInner>>,
    index: u32,
}

struct BlockAllocatorInner {
    blocks: Vec<[f32; BLOCK_SIZE]>,
    free_blocks: Vec<u32>,
}

impl BlockAllocator {
    pub fn new(capacity: usize) -> Self {
        BlockAllocator {
            inner: Rc::new(UnsafeCell::new(BlockAllocatorInner {
                blocks: Vec::with_capacity(capacity),
                free_blocks: Vec::with_capacity(capacity),
            })),
        }
    }

    /// Allocate a block, which is usually *not* zeroed.
    ///
    /// Zeroing is left to the caller because this is an expensive operation.  In debug builds, the returned buffer is guaranteed to contain random data.
    pub fn allocate_block(&mut self) -> AllocatedBlock {
        let inner = unsafe { self.inner.get().as_mut().unwrap_unchecked() };
        let index = match inner.free_blocks.pop() {
            Some(i) => i,
            None => {
                inner.blocks.push([0.0f32; BLOCK_SIZE]);
                (inner.blocks.len() - 1) as u32
            }
        };

        let mut ret = AllocatedBlock {
            allocator: self.inner.clone(),
            index,
        };

        // We'll want to speed this up, but it's fine for now.
        #[cfg(debug_assertions)]
        {
            let mut rgen =
                crate::fast_xoroshiro::FastXoroshiro128PlusPlus::<1>::new_seeded(index as u64);
            let loc = self.deref_block(&mut ret);
            for o in loc.iter_mut() {
                let rval = rgen.gen_u64() as u16;
                *o = 1.0 - (rval as f32) / (u16::MAX as f32) * 2.0;
            }
        }

        ret
    }

    /// Resolve the given allocated block to a mutable array.
    // This takes `&Self` and returns `&mut` because [AllocatedBlock] is a type-level proof that there is only one
    // outstanding reference.  The shared reference prevents allocating new blocks, thus ensuring that data  isn't
    // moved.
    pub fn deref_block<'a>(&self, block: &'a mut AllocatedBlock) -> &'a mut [f32; BLOCK_SIZE] {
        let raw_blocks = {
            // OK: only one mutable reference to inner.
            let inner = unsafe { self.inner.get().as_mut().unwrap_unchecked() };
            // OK: we go from inner to a raw pointer to the backing data, and then drop all our mutable references.
            assert!((block.index as usize) < inner.blocks.len());
            assert!(!inner.blocks.is_empty());
            inner.blocks.as_mut_ptr()
        };

        // Ok: our new block is synthesized from a raw pointer, and we have a type-level proof that this pointer is
        // unique.
        unsafe {
            raw_blocks
                .add(block.index as usize)
                .as_mut()
                .unwrap_unchecked()
        }
    }
}

impl Drop for AllocatedBlock {
    fn drop(&mut self) {
        // This is safe because reading buffers never touches the free block stack.
        unsafe {
            self.allocator
                .get()
                .as_mut()
                .unwrap_unchecked()
                .free_blocks
                .push(self.index)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        let mut allocator = BlockAllocator::new(5);

        let mut b1 = allocator.allocate_block();
        let mut b2 = allocator.allocate_block();
        let mut b3 = allocator.allocate_block();

        {
            let s1 = allocator.deref_block(&mut b1);
            let s2 = allocator.deref_block(&mut b2);
            let s3 = allocator.deref_block(&mut b3);

            s1.fill(1.0);
            s2.fill(2.0);
            s3.fill(3.0);
        }

        let inner = unsafe { allocator.inner.get().as_mut().unwrap() };
        assert_eq!(inner.blocks[0], [1.0; BLOCK_SIZE]);
        assert_eq!(inner.blocks[1], [2.0; BLOCK_SIZE]);
        assert_eq!(inner.blocks[2], [3.0; BLOCK_SIZE]);
    }

    #[test]
    fn test_reusing() {
        let mut allocator = BlockAllocator::new(3);

        let b1 = allocator.allocate_block();
        let b2 = allocator.allocate_block();
        let b3 = allocator.allocate_block();

        assert_eq!([b1.index, b2.index, b3.index], [0, 1, 2]);
        std::mem::drop(b2);
        let b4 = allocator.allocate_block();
        assert_eq!(b4.index, 1);
    }
}

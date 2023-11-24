//! Primitives for synchronization in audio `texts.
//!
//! This crate provides mechanisms whereby an audio thread can communicate with other threads without ever entering the
//! kernel or blocking for an unbounded amount of time.  Generally, synchronization primitives optimize for memory usage
//! or performance, but the important feature for an audio application is that the audio half of a communication process
//! is never blocked.  As an example of something which is seemingly safe but turns out not to be under the hood,
//! Crossbeam's unbounded channels and queues deallocate on the receiving side, even when using operations which
//! ostensibly don't block.

#[cfg(not(loom))]
pub mod concurrent_slab;
pub mod fast_thread_id;
pub mod fixed_size_pool;
pub mod generational_atomic;
pub mod mpsc_counter;
pub mod optional_atomic_u32;
pub mod prepend_only_list;
pub mod spsc_ring;
mod sync;

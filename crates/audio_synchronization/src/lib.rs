//! Primitives for synchronization in audio contexts.
//!
//! This crate provides mechanisms whereby an audio thread can communicate with other threads without ever entering the
//! kernel or blocking for an unbounded amount of time.  Generally, synchronization primitives optimize for memory usage
//! or performance, but the important feature for an audio application is that the audio half of a communication process
//! is never blocked.  As an example of something which is seemingly safe but turns out not to be under the hood,
//! Crossbeam's unbounded channels and queues deallocate on the receiving side, even when using operations which
//! ostensibly don't block.

pub mod spsc_queue;
mod sync;

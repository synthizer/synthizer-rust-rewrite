//! Utility for batching frame-by-frame operations into block operations for performance.
//!
//! This module provides `FrameBatcher`, a utility that allows signals to maintain a
//! frame-based API while internally processing blocks for efficiency. This is particularly
//! useful for signals that need to minimize virtual dispatch overhead (like boxed signals)
//! or other per-frame costs.
//!
//! # Example
//!
//! ```ignore
//! use synthizer::signals::FrameBatcher;
//!
//! struct MySignalState {
//!     batcher: FrameBatcher<f64, f64>,
//!     some_context: SomeType,
//! }
//!
//! impl Signal for MySignal {
//!     fn tick_frame(ctx: &SignalExecutionContext, input: f64, state: &mut MySignalState) -> f64 {
//!         // The closure can capture whatever it needs from state or context
//!         state.batcher.process_frame(input, |inputs, outputs| {
//!             // Process entire block at once when ready
//!             for (input, output) in inputs.iter().zip(outputs.iter_mut()) {
//!                 *output = expensive_operation(*input, &state.some_context);
//!             }
//!         })
//!     }
//! ```
//!
//! The batcher automatically handles:
//! - Collecting input frames until enough are available for batch processing
//! - Buffering output frames and returning them one at a time
//! - Resetting state at block boundaries
use crate::config;

/// A utility that batches single-frame operations into block operations.
///
/// This is designed to be zero-cost when inlined, allowing signals to maintain
/// a frame-based API while internally processing blocks for efficiency.
///
/// Uses stack-allocated buffers via ArrayVec to avoid heap allocations.
///
/// The batcher assumes it processes exactly BLOCK_SIZE frames per block.
pub struct FrameBatcher<I, O> {
    /// Buffer for collecting input frames
    input_buffer: [I; config::BLOCK_SIZE],

    /// Buffer for storing output frames  
    output_buffer: [O; 128],

    /// Current position in both buffers
    pos: usize,
}

impl<I: Copy + Default, O: Copy + Default> FrameBatcher<I, O> {
    /// Create a new frame batcher
    pub fn new() -> Self {
        Self {
            input_buffer: std::array::from_fn(|_| I::default()),
            output_buffer: std::array::from_fn(|_| O::default()),
            pos: 0,
        }
    }

    /// Process a single frame, batching internally.
    ///
    /// The closure can capture whatever context it needs. When the batch is full (BLOCK_SIZE frames), the processor is
    /// called with input and output slices.
    #[inline(always)]
    pub fn process_frame<F>(&mut self, input: I, mut processor: F) -> O
    where
        F: FnMut(&[I], &mut [O]),
    {
        self.input_buffer[self.pos] = input;
        let res = self.output_buffer[self.pos];
        self.pos += 1;

        if self.pos == config::BLOCK_SIZE {
            processor(&self.input_buffer[..], &mut self.output_buffer[..]);

            self.pos = 0;
        }

        res
    }
}

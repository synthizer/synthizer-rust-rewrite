# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Common Development Commands

### Build Commands
```bash
# Check all crates for compilation errors
cargo check --workspace

# Build all crates
cargo build --workspace

# Build in release mode
cargo build --release

# Check a specific crate
cargo check -p synthizer
```

### Test Commands
```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p synthizer

# Run a specific test
cargo test test_name

# Run tests with verbose output
cargo test -- --nocapture
```

### Linting and Formatting
```bash
# Format code
cargo fmt

# Run clippy linter
cargo clippy --workspace -- -D warnings

# Check formatting without modifying
cargo fmt -- --check
```

### Running Examples
```bash
# Run an example from the synthizer crate
cargo run --example binaural_beats
cargo run --example delay_line
cargo run --example media_player
cargo run --example sin_chords
```

## Architecture Overview

This is a Rust rewrite of Synthizer, an audio synthesis library. The codebase is organized as a workspace with multiple crates:

### Core Crates

1. **synthizer** (`crates/synthizer/`) - The main audio synthesis library
   - Signal processing framework based on the `Signal` trait in `core_traits.rs`
   - Block-based audio processing with configurable block sizes
   - Support for various audio formats and channel conversions
   - Media playback through the `sample_sources` module
   - Effects and filters (biquad filters, delay lines, etc.)

2. **audio_synchronization** (`crates/audio_synchronization/`) - Lock-free data structures for audio processing
   - Concurrent slab allocator
   - SPSC ring buffers
   - Atomic counters and synchronization primitives
   - Designed for real-time audio thread safety

3. **synthizer_miniaudio** (`crates/miniaudio/`) - Audio device I/O bindings
   - Wrapper around miniaudio C library
   - Device enumeration and management
   - Audio input/output handling

4. **synthizer_protos** (`crates/protos/`) - Protocol buffer definitions
   - HRTF (Head-Related Transfer Function) data structures
   - Used for 3D audio spatialization

### Key Design Patterns

1. **Signal Processing Chain**: The core abstraction is the `Signal` trait which processes audio in blocks. Signals can be composed using combinators like `map`, `and_then`, `join`, etc.

2. **Value Providers**: A zero-cost abstraction for providing values (samples, frames) that can be computed on-demand or pre-computed.

3. **Audio Frames**: Generic audio frame types that support different channel counts without heap allocation.

4. **Lock-Free Audio Thread**: The library is designed for real-time audio processing with lock-free data structures and careful memory management.

5. **Block Processing**: Audio is processed in fixed-size blocks (configurable via `config::BLOCK_SIZE`) for efficiency.

### Important Conventions

- The codebase uses `#![allow(dead_code)]` during development
- Tests use `#[cfg(test)]` modules and the `proptest` framework for property-based testing
- The project uses Rust 1.83 with rustfmt and clippy configured
- SIMD and performance optimizations are planned but not yet fully implemented
- Custom allocators and memory management strategies are used for real-time performance
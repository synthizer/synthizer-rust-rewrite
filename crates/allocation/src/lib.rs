//! A crate for allocating many objects of the same type efficiently.
//!
//! Rust's [std::sync::Arc] has two deficiencies:
//!
//! - It is not possible to use custom allocators, save on nightly.
//! - It is not possible to do things such as projecting to fields, e.g. getting an Arc to a subfield of a larger struct
//!   that represents a "base class".
//!
//! This crate aims to solve both problems, plus the problem wherein something for audio needs to be able to defer
//! allocations to background threads in a realtime-safe manner.
//!
//! This is a work in progress: more will be added as we need it and not all features yet exist.
mod allocation_page;
mod allocation_strategies;
mod allocator;
mod shared_ptr;

pub use allocator::*;
pub use shared_ptr::*;

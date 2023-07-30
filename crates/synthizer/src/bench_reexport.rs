//! Reexports private APIs for benchmarks.
//!
//! This is technically public, but `doc(hidden)`.

pub mod unique_id {
    pub use crate::unique_id::*;
}

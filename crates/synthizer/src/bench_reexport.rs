//! Reexports private APIs for benchmarks.
//!
//! This is technically public, but `doc(hidden)`.

pub mod data_structures {
    pub mod stager {
        pub use crate::data_structures::stager::*;
    }

    pub mod edgemap {
        pub use crate::data_structures::edgemap::*;
    }
}

pub mod unique_id {
    pub use crate::unique_id::*;
}

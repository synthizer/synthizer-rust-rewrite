//! Synthizer protocol buffer definitions.
//!
//! These have to be pulled out so that Synthizer's build script can use them.
pub mod hrtf {
    include!(concat!(env!("OUT_DIR"), "/hrtf.rs"));
}

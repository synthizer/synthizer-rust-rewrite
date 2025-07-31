#![allow(dead_code)]

#[macro_use]
mod logging;

mod array_utils;
mod audio_frames;
pub mod biquad;
pub mod bus;
pub mod chain;
pub mod chain_mathops;
mod channel_conversion;
mod channel_format;
#[cfg(test)]
mod close_floats;
mod config;
mod context;
pub mod core_traits;
pub(crate) mod cpal_device;
mod data_structures;
mod db;
mod error;
pub mod fast_xoroshiro;
mod handle;
mod is_audio_thread;
mod loop_spec;
mod mark_dropped;
mod mount_point;
mod option_recycler;
mod program;
mod resampling;
pub mod sample_sources;
pub mod signals;
pub mod synthesizer;
mod unique_id;
mod unsafe_utils;
mod wavetable;
mod worker_pool;

pub use bus::BusHandle;
pub use chain::Chain;
pub use channel_format::ChannelFormat;
pub use config::SR;
pub use db::DbExt;
pub use error::{Error, Result};
pub use handle::Handle;
pub use loop_spec::LoopSpec;
pub use program::Program;
pub use signals::DelayLineHandle;
pub use synthesizer::Synthesizer;

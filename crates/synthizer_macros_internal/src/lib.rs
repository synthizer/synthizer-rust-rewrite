//! This crate contains macros for Synthizer's internal use.
//! 
//! Synthizer has a lot of internal boilerplate by virtue of needing to be realtime-safe, for example declaring commands
//! which will go cross-thread for every possible function which might need to run on the audio thread.  It also has a
//! lot of boilerplate abstraction, for example understanding the concept of a property and building node descriptors.
//! Rather than type this tens of times and refactor them all tens of times per change, we have this crate to help us
//! out.

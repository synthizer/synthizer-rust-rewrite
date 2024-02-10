//! This module contains a bunch of commands used by more than one node.
use crate::LoopSpec;

/// Command representing configuration of loops.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct SetLoopConfigCommand(pub(crate) LoopSpec);

/// Command representing a seek to a given position in samples.
///
/// Seeks are in the samplerate of the underlying source.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct SeekCommand(pub(crate) u64);

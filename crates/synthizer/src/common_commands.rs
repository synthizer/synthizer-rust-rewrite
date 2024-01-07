//! This module contains a bunch of commands used by more than one node.
use crate::LoopSpec;

/// Command representing configuriation of loops.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct SetLoopConfigCommand(pub(crate) LoopSpec);

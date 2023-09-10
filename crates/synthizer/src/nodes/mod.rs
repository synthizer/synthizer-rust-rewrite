pub mod audio_output;
pub mod descriptor;
pub mod traits;
pub mod trig;

pub use audio_output::AudioOutputNodeHandle;
pub use descriptor::*;
pub use traits::*;
pub use trig::TrigWaveformNodeHandle;

use std::borrow::Cow;

use arrayvec::ArrayVec;

use audio_synchronization::concurrent_slab::ExclusiveSlabRef;

use crate::config::*;
use crate::data_structures::AddOnlyBlock;
use crate::data_structures::AllocatedBlock;

/// An output destination.
pub(crate) enum OutputDestination<'a> {
    /// This output is going to the specified blocks, which will match what the descriptor requested.
    ///
    /// The blocks may not be zeroed, and should be added to instead.
    Block(ArrayVec<AddOnlyBlock<'a>, MAX_CHANNELS>),
}

pub(crate) type OutputsByIndex<'a> = arrayvec::ArrayVec<OutputDestination<'a>, MAX_OUTPUTS>;

/// Inputs by index. These are slices from the inputs arrays in ServerImpl's memoized_outputs hashmap.
pub(crate) type InputsByIndex<'a> = arrayvec::ArrayVec<&'a [AllocatedBlock], MAX_INPUTS>;

/// This enum holds ExclusiveSlabRefs to all node types we support.
///
/// We don't want to use Box in this case because that would be `Box<ExclusiveSlabRef<T>>` (except erased), which is a
/// double pointer and a heap allocation.
///
/// The weird name is because we wish to reserve the name NodeHandle for the external API.
#[enum_dispatch::enum_dispatch(ErasedNode)]
pub(crate) enum ConcreteNodeHandle {
    TrigWaveform(ExclusiveSlabRef<trig::TrigWaveformNode>),
    AudioOutput(ExclusiveSlabRef<audio_output::AudioOutputNode>),
}

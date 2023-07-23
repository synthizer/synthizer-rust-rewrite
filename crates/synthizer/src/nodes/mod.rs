pub mod descriptor;
pub mod traits;
pub mod trig;

pub use descriptor::*;
pub use traits::*;

use arrayvec::ArrayVec;

use audio_synchronization::concurrent_slab::ExclusiveSlabRef;

use crate::config::*;
use crate::data_structures::AddOnlyBlock;

/// An output destination.
pub(crate) enum OutputDestination<'a> {
    /// This output is going to the specified blocks, which will match what the descriptor requested.
    ///
    /// The blocks may not be zeroed, and should be added to instead.
    Block(ArrayVec<AddOnlyBlock<'a>, MAX_CHANNELS>),
}

pub(crate) type OutputsByIndex<'a> = arrayvec::ArrayVec<OutputDestination<'a>, 16>;

/// This enum holds ExclusiveSlabRefs to all node types we support.
///
/// We don't want to use Box in this case because that would be `Box<ExclusiveSlabRef<T>>` (except erased), which is a double pointer and a heap allocation.
#[enum_dispatch::enum_dispatch(ErasedNodeAt)]
pub enum NodeHandle {
    TrigWaveform(ExclusiveSlabRef<trig::TrigWaveform>),
}

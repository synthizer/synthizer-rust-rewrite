use std::borrow::Cow;
use std::ops::{Deref, DerefMut};

use arrayvec::ArrayVec;

use audio_synchronization::concurrent_slab::ExclusiveSlabRef;

use crate::config::*;
use crate::data_structures::AddOnlyBlock;
use crate::data_structures::{AllocatedBlock, BlockAllocator, ExecutableNodeContext};
use crate::nodes::OutputsByIndex;
use crate::nodes::*;
use crate::server::implementation::AudioThreadServerServices;

/// A trait representing a set of outputs.
pub(crate) trait FromOutputSlice<'a> {
    fn to_named_outputs<'b>(outputs: &'b mut OutputsByIndex<'a>) -> Self;
}

pub(crate) struct NodeExecutionContext<'a, 'b, N: NodeAt + ?Sized> {
    pub(crate) outputs: &'a mut N::Outputs<'b>,
    pub(crate) services: &'a mut AudioThreadServerServices,

    /// The speakers, or output of the graph, etc.
    ///
    /// Used by nodes which know how to write data to the destination.  This slice is always equal in length to the
    /// graph's number of output channels, and expects to be (possibly) filled with data by the node.  Note that these
    /// should be added to: wrapping them in [AddOnlyBlock] is complex, and this is rarely used so that's not worth
    /// bothering with.
    ///
    /// Unfortunately there isn't a good name for this.
    pub(crate) speaker_outputs: &'a mut [AllocatedBlock],
}

/// Results from executing a node.
#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd, derive_more::IsVariant)]
pub(crate) enum NodeExecutionOutcome {
    /// The usual case: this node output some audio.
    SentAudio,
}

/// This context is erased: we figure out the generic parameters in gather_and_execute.
pub(crate) struct ErasedExecutionContext<'a> {
    pub(crate) services: &'a mut AudioThreadServerServices,
    pub(crate) speaker_outputs: &'a mut [AllocatedBlock],
    pub(crate) speaker_format: &'a crate::channel_format::ChannelFormat,
}

/// Trait representing the part of a node which runs on the audio output thread.
///
/// Nodes generally consist of two pieces: a public, user-facing piece containing a handle, and a private, server-stored
/// piece which holds state for and is executed on the audio thread.  This trait encapsulates the audio thread component
/// (thus At).
pub(crate) trait NodeAt {
    type Outputs<'a>: FromOutputSlice<'a> + Sized;

    /// Run this node.
    fn execute<'a>(
        &'a mut self,
        context: &'a mut NodeExecutionContext<Self>,
    ) -> NodeExecutionOutcome;

    /// Describe this node.
    ///
    /// This function will be called after node setup.  Nodes should never change their descriptors at runtime.  Will be
    /// called once after node construction, not on the audio thread.
    fn describe(&self) -> Cow<'static, NodeDescriptor>;

    /// Gather all state needed for this node and execute it.  Implementors of this trait should use the default impl.
    fn gather_and_execute(&mut self, context: &mut ErasedExecutionContext) {
        // For now, this forces "output" to the speakers, and assumes only one output. That's good enough to get a sine
        // wave going, as well as a few other foundational pieces like buffer generators and noise.

        // Todo: memoize.
        let desc = self.describe();
        let needed_channels = desc.outputs[0].channel_format.get_channel_count();

        let mut output_blocks = (0..needed_channels.get())
            .map(|_| context.services.block_allocator.allocate_block())
            .map(|mut x| {
                x.fill(0.0f32);
                x
            })
            .collect::<ArrayVec<_, MAX_CHANNELS>>();

        {
            let add_only = output_blocks
                .iter_mut()
                .map(AddOnlyBlock::new)
                .collect::<ArrayVec<_, MAX_CHANNELS>>();
            let output = OutputDestination::Block(add_only);

            let mut output_smallvec = std::iter::once(output).collect();
            let mut outputs = Self::Outputs::to_named_outputs(&mut output_smallvec);

            let mut ctx = NodeExecutionContext {
                outputs: &mut outputs,
                services: context.services,
                speaker_outputs: context.speaker_outputs,
            };

            self.execute(&mut ctx);
        }

        crate::channel_conversion::convert_channels(
            &desc.outputs[0].channel_format,
            context.speaker_format,
            &output_blocks[..],
            context.speaker_outputs,
            // In this context, always add.
            true,
        );
    }
}

/// AN erased node.
///
/// This is an object safe node, which can be stored in e.g. Box.
#[enum_dispatch::enum_dispatch]
pub(crate) trait ErasedNodeAt {
    fn execute_erased(&mut self, context: &mut ErasedExecutionContext);
}

impl<T: NodeAt> ErasedNodeAt for T {
    fn execute_erased(&mut self, context: &mut ErasedExecutionContext) {
        self.gather_and_execute(context)
    }
}

impl<T: ErasedNodeAt> ErasedNodeAt for ExclusiveSlabRef<T> {
    fn execute_erased(&mut self, context: &mut ErasedExecutionContext) {
        self.deref_mut().execute_erased(context);
    }
}

use std::borrow::Cow;
use std::ops::{Deref, DerefMut};

use arrayvec::ArrayVec;

use audio_synchronization::concurrent_slab::ExclusiveSlabRef;

use crate::channel_format::ChannelFormat;
use crate::config::*;
use crate::data_structures::AddOnlyBlock;
use crate::data_structures::{AllocatedBlock, BlockAllocator};
use crate::nodes::OutputsByIndex;
use crate::nodes::*;
use crate::server::implementation::AudioThreadServerServices;
use crate::unique_id::UniqueId;

/// A trait representing a set of outputs.
pub(crate) trait ToNamedOutputs<'a> {
    fn to_named_outputs<'b>(outputs: &'b mut OutputsByIndex<'a>) -> Self;
}

/// A trait representing a set of inputs.
pub(crate) trait ToNamedInputs<'a> {
    fn to_named_inputs<'b>(inputs: &'b mut InputsByIndex<'a>) -> Self;
}

impl<'a> ToNamedOutputs<'a> for () {
    fn to_named_outputs<'b>(_outputs: &'b mut OutputsByIndex<'a>) -> Self {}
}

impl<'a> ToNamedInputs<'a> for () {
    fn to_named_inputs<'b>(_inputs: &'b mut InputsByIndex<'a>) -> Self {}
}

pub(crate) struct NodeExecutionContext<'a, 'b, N: NodeAt + ?Sized> {
    pub(crate) inputs: &'a N::Inputs<'b>,
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
    pub(crate) speaker_format: &'a ChannelFormat,
}

/// Results from executing a node.
#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd, derive_more::IsVariant)]
pub(crate) enum NodeExecutionOutcome {
    /// The usual case: this node output some audio.
    SentAudio,
}

/// An input for a given node.
pub(crate) struct IndividualInputData {
    pub(crate) format: ChannelFormat,
    pub(crate) data: ArrayVec<AllocatedBlock, MAX_CHANNELS>,
}

#[derive(Default)]
pub(crate) struct NodeInputsData {
    pub(crate) inputs: [Option<IndividualInputData>; MAX_INPUTS],
}

/// This context is erased: we figure out the generic parameters in gather_and_execute.
pub(crate) struct ErasedExecutionContext<'a> {
    pub(crate) id: UniqueId,

    pub(crate) services: &'a mut AudioThreadServerServices,

    /// The memoized descriptors.  This can't be under services because we need to be able to grab them immutably
    /// without cloning, since some (such as AudioOutputNode) would clone data on the heap.
    pub(crate) descriptors: &'a ahash::HashMap<UniqueId, Cow<'static, NodeDescriptor>>,

    /// the graph this node is in.
    ///
    /// This is *not* necessarily the same as the server's root graph.
    pub(crate) graph: &'a crate::data_structures::Graph,

    pub(crate) speaker_outputs: &'a mut [AllocatedBlock],
    pub(crate) speaker_format: &'a crate::channel_format::ChannelFormat,
}

pub(crate) trait HasNodeDescriptor {
    type Outputs<'a>: ToNamedOutputs<'a> + Sized;
    type Inputs<'a>: ToNamedInputs<'a> + Sized;

    /// Describe this node.
    ///
    /// This function will be called after node setup.  Nodes should never change their descriptors at runtime.  Will be
    /// called once after node construction, not on the audio thread.
    fn describe(&self) -> Cow<'static, NodeDescriptor>;
}

/// Trait representing the part of a node which runs on the audio output thread.
///
/// Nodes generally consist of two pieces: a public, user-facing piece containing a handle, and a private, server-stored
/// piece which holds state for and is executed on the audio thread.  This trait encapsulates the audio thread component
/// (thus At).
pub(crate) trait NodeAt: HasNodeDescriptor {
    /// Run this node.
    fn execute<'a>(
        &'a mut self,
        context: &'a mut NodeExecutionContext<Self>,
    ) -> NodeExecutionOutcome;

    /// Gather all state needed for this node and execute it.  Implementors of this trait should use the default impl.
    fn gather_and_execute(&mut self, context: &mut ErasedExecutionContext) {
        let desc = context
            .descriptors
            .get(&context.id)
            .expect("Descriptor should be registered with the node");

        // For simplicity we also currently assume at most one output.
        let needed_output_channels = desc
            .outputs
            .get(0)
            .map(|x| x.channel_format.get_channel_count().get())
            .unwrap_or(0);

        let mut output_blocks_raw = ArrayVec::<AllocatedBlock, MAX_CHANNELS>::new();

        for _ in 0..needed_output_channels {
            let mut b = context.services.block_allocator.allocate_block();
            b.fill(0.0);
            output_blocks_raw.push(b);
        }

        {
            let output_blocks: ArrayVec<AddOnlyBlock, MAX_CHANNELS> = output_blocks_raw
                .iter_mut()
                .map(AddOnlyBlock::new)
                .collect();

            // We will either get some inputs from the hashmap, or we will need to synthesize some zeroed blocks.  We will
            // optimize synthesis of zeroed blocks later.
            let mut synthesized_input_blocks: ArrayVec<AllocatedBlock, MAX_CHANNELS> =
                ArrayVec::new();
            let maybe_inputs = context.services.input_data.remove(&context.id);

            // To get us off the ground, we pretend there is only at most one input ever.
            let mut input_av = maybe_inputs
                .as_ref()
                .and_then(|x| x.inputs[0].as_ref())
                .map(|x| {
                    let mut out = ArrayVec::new();
                    out.push(&x.data[..]);
                    out
                })
                .unwrap_or_else(|| {
                    let mut out = ArrayVec::new();

                    if !desc.inputs.is_empty() {
                        for _ in 0..desc.inputs[0].channel_format.get_channel_count().get() {
                            let mut b = context.services.block_allocator.allocate_block();
                            b.fill(0.0);
                            synthesized_input_blocks.push(b);
                        }
                    }

                    out.push(&synthesized_input_blocks[..]);
                    out
                });

            let mut output_dests = ArrayVec::new();
            if !output_blocks.is_empty() {
                output_dests.push(OutputDestination::Block(output_blocks))
            }

            let inputs = Self::Inputs::to_named_inputs(&mut input_av);
            let mut outputs = Self::Outputs::to_named_outputs(&mut output_dests);

            let mut ctx = NodeExecutionContext {
                inputs: &inputs,
                outputs: &mut outputs,
                services: context.services,
                speaker_outputs: context.speaker_outputs,
                speaker_format: context.speaker_format,
            };

            self.execute(&mut ctx);
        }

        // Now we go through the nodes that we might need to send data to, and downmix them.
        context
            .graph
            .iter_outgoing(context.id)
            .for_each(move |edge| {
                let incoming_desc = context
                    .descriptors
                    .get(&edge.incoming_node)
                    .expect("Nodes should be registered");

                let mixing_target = context
                    .services
                    .input_data
                    .entry(edge.incoming_node)
                    .or_default();

                if let Some(input) = &mut mixing_target.inputs[edge.incoming_index] {
                    // This is a mix from the data we have, currently in our one output, to the target.
                    crate::channel_conversion::convert_channels(
                        &desc.outputs[edge.outgoing_index].channel_format,
                        &input.format,
                        &output_blocks_raw,
                        &mut input.data,
                        // In this case, there was already some mixed data.
                        true,
                    );
                } else {
                    let mut data: ArrayVec<AllocatedBlock, MAX_CHANNELS> = (0..incoming_desc
                        .inputs[edge.incoming_index]
                        .channel_format
                        .get_channel_count()
                        .get())
                        .map(|_| context.services.block_allocator.allocate_block())
                        .collect();
                    crate::channel_conversion::convert_channels(
                        &desc.outputs[edge.outgoing_index].channel_format,
                        &incoming_desc.inputs[edge.incoming_index].channel_format,
                        &output_blocks_raw,
                        &mut data,
                        // data is uninitialized, and should be filled directly, so no adding.
                        false,
                    );
                    mixing_target.inputs[edge.incoming_index] = Some(IndividualInputData {
                        format: incoming_desc.inputs[edge.incoming_index]
                            .channel_format
                            .clone(),
                        data,
                    });
                }
            });
    }
}

pub(crate) trait Node: HasNodeDescriptor + NodeAt {}
impl<T: HasNodeDescriptor + NodeAt> Node for T {}

/// AN erased node.
///
/// This is an object safe node, which can be stored in e.g. Box.
#[enum_dispatch::enum_dispatch]
pub(crate) trait ErasedNode {
    fn describe_erased(&self) -> Cow<'static, NodeDescriptor>;
    fn execute_erased(&mut self, context: &mut ErasedExecutionContext);
}

impl<T: Node> ErasedNode for T {
    fn describe_erased(&self) -> Cow<'static, NodeDescriptor> {
        self.describe()
    }

    fn execute_erased(&mut self, context: &mut ErasedExecutionContext) {
        self.gather_and_execute(context)
    }
}

impl<T: Send + Sync + ErasedNode> ErasedNode for ExclusiveSlabRef<T> {
    fn describe_erased(&self) -> Cow<'static, NodeDescriptor> {
        self.deref().describe_erased()
    }

    fn execute_erased(&mut self, context: &mut ErasedExecutionContext) {
        self.deref_mut().execute_erased(context);
    }
}

mod sealed_node_handle {
    use super::*;

    pub trait NodeHandleSealed {
        fn get_id(&self) -> UniqueId;
    }
}
pub(crate) use sealed_node_handle::*;

/// Trait representing a node.
///
/// Nodes have a few capabilities, most notably the ability to connect to each other in a graph.  This trait is
/// implemented for every handle to a node, and allows using them with graph infrastructure and in other palces where
/// Synthizer wishes to have a node.
pub trait NodeHandle: Clone + Send + Sync + NodeHandleSealed + 'static {}

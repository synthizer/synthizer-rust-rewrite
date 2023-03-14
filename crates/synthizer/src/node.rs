use std::borrow::Cow;

use crate::node_descriptor::NodeDescriptor;

/// An output destination.
pub(crate) enum OutputDestination<'a> {
    /// This output is going to the specified slice.
    ///
    /// The slice will be zeroed and exactly `channels * BLOCK_SIZE` in length, where `channels` comes from the
    /// descriptor for this node.
    Slice(&'a mut [f32]),
}

pub(crate) type OutputsByIndex<'a> = arrayvec::ArrayVec<&'a mut OutputDestination<'a>, 16>;

/// A trait representing a set of outputs.
///
/// Nodes which have no outputs can use `()`.
pub(crate) trait FromOutputSlice {
    type NamedOutputs<'a>;

    fn to_named_outputs(outputs: OutputsByIndex) -> Self::NamedOutputs<'_>;
}

impl FromOutputSlice for () {
    type NamedOutputs<'a> = ();
    fn to_named_outputs(_outputs: OutputsByIndex) {}
}

pub(crate) struct NodeExecutionContext<'a, N: Node + ?Sized> {
    pub(crate) outputs: &'a mut N::Outputs<'a>,
    pub(crate) state: &'a mut N::State,
}

/// Results from executing a node.
#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd, derive_more::IsVariant)]
pub(crate) enum NodeExecutionOutcome {
    /// The usual case: this node output some audio.
    SentAudio,
}

/// Trait representing a node, a piece of machinery running on the audio output thread.
///
/// Nodes do not have a `self` parameter and are effectively only labels.  Their existance on the audio thread gets
/// state from the [Node::State] type, which is injected into the context passed to [Node::execute].  This allows nodes
/// to be allocated disjointly and materialized on demand.  The actual pieces of a node are in different containers and
/// are represented by the associated types on this trait.
pub(crate) trait Node {
    type Outputs<'a>: FromOutputSlice + Sized;
    type State: Send + Sized;

    fn execute(context: &mut NodeExecutionContext<Self>) -> NodeExecutionOutcome;

    /// Describe this node.
    ///
    /// This function will be called with the state after node setup.  Nodes should never change their descriptors at
    /// runtime.
    fn describe(state: &Self::State) -> Cow<'static, NodeDescriptor>;
}

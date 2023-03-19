use crate::data_structures::edgemap::*;
use crate::unique_id::UniqueId;

/// The output end of a connection.
///
/// Generalizes to [UniqueId]s representing nodes.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct NodeOutput {
    node: UniqueId,
    output: usize,
}

/// The input end of a connection.
///
/// Generalizes to [UniqueId]s representing nodes.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct NodeInput {
    node: UniqueId,
    input: usize,
}

impl GeneralizedEndpoint<UniqueId> for NodeOutput {
    fn generalize(&self) -> &UniqueId {
        &self.node
    }
}

impl GeneralizedEndpoint<UniqueId> for NodeInput {
    fn generalize(&self) -> &UniqueId {
        &self.node
    }
}

/// An edge in a [NodeDependencyGraph].
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
struct NodeDependencyGraphEdge {
    output: NodeOutput,
    input: NodeInput,
}

impl Edge for NodeDependencyGraphEdge {
    type Outgoing = NodeOutput;
    type Incoming = NodeInput;

    fn get_incoming(&self) -> &Self::Incoming {
        &self.input
    }

    fn get_outgoing(&self) -> &Self::Outgoing {
        &self.output
    }
}

/// A graph of nodes.
///
/// This is the main data structure for planning.  It does not contain information on node formats or states, just a
/// dependency graph.
pub(crate) struct NodeDependencyGraph {
    edges: EdgeMap<NodeDependencyGraphEdge>,
}

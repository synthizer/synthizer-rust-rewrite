use im::{HashMap, HashSet};

use crate::unique_id::UniqueId;

/// Describes an outgoing or incoming edge.
///
/// Both the outgoing and incoming nodes get a copy of this descriptor for an edge, which allows easy bidirectional
/// lookup.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct EdgeDescriptor {
    pub(crate) outgoing_node: UniqueId,
    pub(crate) outgoing_index: usize,
    pub(crate) incoming_node: UniqueId,
    pub(crate) incoming_index: usize,
}

#[derive(Clone, Debug, Default)]
struct GraphEntry {
    incoming: HashSet<EdgeDescriptor>,
    outgoing: HashSet<EdgeDescriptor>,
}

/// A graph backed by im.
///
/// This graph is updated via cloning the internal im types and can itself be cloned.  It is passed to the audio thread
/// via said cloning, on an as-needed basis.
///
/// The nodes are always [UniqueId].  The edges are always ([UniqueId], output and/or input) tuples.
#[derive(Clone, Debug)]
pub(crate) struct Graph {
    nodes: HashMap<UniqueId, GraphEntry>,
}

impl Graph {
    pub fn new() -> Self {
        Self {
            nodes: Default::default(),
        }
    }

    /// Insert a node with no connections.
    ///
    /// Panics if the node already existed.
    pub(crate) fn register_node(&mut self, node: UniqueId) {
        let had = self.nodes.insert(node, Default::default()).is_some();
        assert!(!had, "Attempt to double register a node");
    }

    /// Deregister a node by removing all references to it.
    ///
    /// Panics if the node is not present.
    pub(crate) fn deregister_node(&mut self, node: UniqueId) {
        let node = self.nodes.remove(&node).expect("The node must be present");

        // This duplicates some work since it's possible for nodes to point at different inputs, but is good enough for
        // now.
        //
        // Note that the outgoing set points at (mostly unique) incoming nodes, and will contain at most O(inputs)
        // unique values for the incoming node.  The incoming set points at unique outgoing nodes, and has the same
        // logic.
        for id in node
            .outgoing
            .into_iter()
            // The outgoing set points at unique incoming nodes.
            .map(|x| x.incoming_node)
            // The incoming set points at unique outgoing nodes.
            .chain(node.incoming.into_iter().map(|x| x.incoming_node))
        {
            let node = self.nodes.get_mut(&id).expect("Node must be present");
            node.outgoing
                .retain(|x| x.outgoing_node != id && x.incoming_node != id);
        }
    }

    /// Connect a node to another node.
    ///
    /// If the connection is already present, does nothing.
    ///
    /// Returns true if an edge was added.
    pub(crate) fn connect(
        &mut self,
        outgoing_node: UniqueId,
        outgoing_index: usize,
        incoming_node: UniqueId,
        incoming_index: usize,
    ) -> bool {
        let desc = EdgeDescriptor {
            outgoing_node,
            outgoing_index,
            incoming_node,
            incoming_index,
        };

        let outgoing_ent = self.nodes.entry(outgoing_node).or_default();

        // im likes to clone early, if one adds to sets without checking membership first.
        if outgoing_ent.outgoing.contains(&desc) {
            return false;
        }

        let did_outgoing = outgoing_ent.outgoing.insert(desc.clone()).is_none();
        let did_incoming = self
            .nodes
            .entry(incoming_node)
            .or_default()
            .incoming
            .insert(desc)
            .is_none();

        assert_eq!(did_outgoing, did_incoming);
        true
    }

    /// Iterate over all outgoing edges for a node.
    pub(crate) fn iter_outgoing(
        &self,
        outgoing: UniqueId,
    ) -> impl Iterator<Item = &EdgeDescriptor> {
        self.nodes
            .get(&outgoing.clone())
            .into_iter()
            .flat_map(|x| x.outgoing.iter())
            .map(move |x| {
                debug_assert_eq!(x.outgoing_node, outgoing);
                x
            })
    }

    /// Iterate over all incoming edges for a node.
    pub(crate) fn iter_incoming(
        &self,
        incoming: UniqueId,
    ) -> impl Iterator<Item = &EdgeDescriptor> {
        self.nodes
            .get(&incoming.clone())
            .into_iter()
            .flat_map(|x| x.incoming.iter())
            .map(move |x| {
                debug_assert_eq!(x.incoming_node, incoming);
                x
            })
    }

    /// Recursively visit every node in this graph, in roughly depth-first order.
    ///
    /// This function will call the provided closure on all node ids (not edges) in the graph in an order such that
    /// execution of the ids will execute dependencies first.  This function does not deduplicate, and will call the
    /// closure more than once for the same node if more than one path to that node exists.
    pub(crate) fn traverse_execution_order(&self, mut callback: impl FnMut(&UniqueId)) {
        let mut roots = self
            .nodes
            .iter()
            .filter(|x| x.1.outgoing.is_empty())
            .map(|x| x.0);
        self.traverse_execution_order_recursive(&mut roots, &mut callback);
    }

    fn traverse_execution_order_recursive(
        &self,
        iterator: &mut dyn Iterator<Item = &UniqueId>,
        callback: &mut dyn FnMut(&UniqueId),
    ) {
        iterator.for_each(|x| {
            // First, recurse on all incoming edges of this node, which will execute the dependencies first as needed.
            let node = self.nodes.get(x).expect("This node must be in the graph");
            let mut deps = node.incoming.iter().map(|x| &x.outgoing_node);
            self.traverse_execution_order_recursive(&mut deps, callback);
            // Now we can visit x.
            callback(x);
        });
    }
}

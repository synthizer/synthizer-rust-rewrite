//! Trace signal execution to determine how it may be run and if it is valid.
//!
//! We allow recursion.  This detects that recursion, computes what is covered by it, and determines if it is possible
//! to run a block at once or not.  It also validates that temporal scopes are valid, providing errors if they're not,
//! and performs other program-level validation.
//!
//! The original operations also carry allocated metadata for the cases wherein they are using `Arc<Any>`, e.g. actual
//! storage for a slot.  These are moved into the synthesizer by the tracer after validity is determined.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use petgraph::prelude::*;

use crate::core_traits::*;
use crate::error::{Error, Result};
use crate::sample_sources::execution::Executor as MediaExecutor;
use crate::unique_id::UniqueId;

enum NodeKind {
    /// This is the signal itself.
    ///
    /// Signals don't get external ids and only run once.  Without other components such as delay lines, it is not
    /// possible to have anything but a DAG (this is enforced by the public API and the implementations; there's no way
    /// to clone a signal and keep identity.  The user allocates their own memory for that).
    Signal,

    ///A node for a slot of given id.
    Slot,

    /// Reference the given media source.
    Media,

    DelayLine,
}

/// Allocations of data which need to be added to the synthesizer later.
pub(crate) enum Allocation {
    Slot(AnyArc),
    Media(Arc<MediaExecutor>),
}

struct Node {
    external_id: Option<UniqueId>,
    kind: NodeKind,
}

/// Records a trace, a directed graph of signal operations.
///
/// This then gets turned into a graph and analyzed for, e.g., cycles.
///
/// The internal graph is currently representing dataflow.  Nodes are signals or the things signals might want to use.
/// Edges are just arrows (no data) where the  edge leads from data source to data consumer.
#[derive(Default)]
pub(crate) struct Tracer {
    graph: DiGraph<Node, ()>,

    /// Unique ids translated to their associated nodes.
    ///
    /// Signals don't appear here.
    translated_ids: HashMap<UniqueId, NodeIndex>,

    /// Pending allocations which will be added to the synthesizer.
    allocations: HashMap<UniqueId, Allocation>,
}

/// Helper trait to perform multiple borrows to get back up to the tracer from a nested stack of signal traces.
trait SingleSignalStackHelper {
    fn get_top(&mut self) -> &mut Tracer;
}

/// A trace of a given signal.
///
/// Signals grab these to trace their execution.  Anything the signal uses is then linked to it, which forms edges as appropriate.  Child signals grab their own guards in turn.
pub(crate) struct SingleSignalTrace<'a> {
    tracer: &'a mut dyn SingleSignalStackHelper,
    id: NodeIndex,
}

/// Characteristics of a traced signal.
pub(crate) struct TracedCharacteristics {
    pub(crate) recursive: bool,
}

impl SingleSignalStackHelper for Tracer {
    fn get_top(&mut self) -> &mut Tracer {
        self
    }
}

impl SingleSignalStackHelper for SingleSignalTrace<'_> {
    fn get_top(&mut self) -> &mut Tracer {
        self.tracer.get_top()
    }
}

impl SingleSignalTrace<'_> {
    pub(crate) fn start_tracing_parent(&mut self) -> SingleSignalTrace<'_> {
        // Id of the new signal
        let id;

        {
            let tracer = self.tracer.get_top();
            id = tracer.graph.add_node(Node {
                external_id: None,
                kind: NodeKind::Signal,
            });

            // Data flows from the parent to this signal.
            tracer.graph.add_edge(id, self.id, ());
        }

        SingleSignalTrace { id, tracer: self }
    }

    /// This signal uses a slot.
    ///
    /// If the slot has not yet previously been allocated, then call the closure to get the slot.
    pub(crate) fn uses_slot<F: FnOnce() -> AnyArc>(&mut self, slot_id: UniqueId, allocator: F) {
        let tracer = self.tracer.get_top();
        tracer.run_allocation_if_needed(slot_id, move || Allocation::Slot(allocator()));

        let slot_node = tracer.translated_ids.entry(slot_id).or_insert_with(|| {
            tracer.graph.add_node(Node {
                external_id: Some(slot_id),
                kind: NodeKind::Slot,
            })
        });

        // the edge is from the slot to the signal.
        tracer.graph.add_edge(*slot_node, self.id, ());
    }

    /// This signal wants to use the given media.
    ///
    /// Errors if another signal is already using this media.
    pub(crate) fn uses_media<F: FnOnce() -> Arc<MediaExecutor>>(
        &mut self,
        media_id: UniqueId,
        allocator: F,
    ) -> Result<()> {
        let tracer = self.tracer.get_top();
        let mut good = false;

        let media_node = *tracer.translated_ids.entry(media_id).or_insert_with(|| {
            // First insert, so all is well.
            good = true;

            tracer.graph.add_node(Node {
                external_id: Some(media_id),
                kind: NodeKind::Media,
            })
        });

        tracer.run_allocation_if_needed(media_id, move || Allocation::Media(allocator()));

        if !good {
            return Err(Error::new_validation_static(
                "Attempt to use the same media twice",
            ));
        }

        // Data flows from media to the signal using it.
        tracer.graph.add_edge(media_node, self.id, ());

        Ok(())
    }

    fn make_delay_line_node(&mut self, id: UniqueId) -> NodeIndex {
        let tracer = self.tracer.get_top();
        *tracer.translated_ids.entry(id).or_insert_with(|| {
            tracer.graph.add_node(Node {
                external_id: Some(id),
                kind: NodeKind::DelayLine,
            })
        })
    }

    pub(crate) fn read_delay_line(&mut self, id: UniqueId) {
        let node = self.make_delay_line_node(id);
        self.tracer.get_top().graph.add_edge(node, self.id, ());
    }

    pub(crate) fn write_delay_line(&mut self, id: UniqueId) {
        let node = self.make_delay_line_node(id);
        self.tracer.get_top().graph.add_edge(self.id, node, ());
    }
}

impl Tracer {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn begin_tracing_signal(&mut self) -> SingleSignalTrace<'_> {
        SingleSignalTrace {
            id: self.graph.add_node(Node {
                external_id: None,
                kind: NodeKind::Signal,
            }),
            tracer: self,
        }
    }

    /// Run the given allocation callback *if* allocations are needed.
    ///
    /// This is used to mock tests.
    fn run_allocation_if_needed<F: FnOnce() -> Allocation>(&mut self, id: UniqueId, allocator: F) {
        self.allocations.entry(id).or_insert_with(allocator);
    }

    /// Analyze the trace.
    ///
    /// This operation is destructive.  Duplicate calls will not work.
    pub(crate) fn validate_and_analyze(&mut self) -> Result<TracedCharacteristics> {
        // To validate cycles, we will exclude all edges which are a resource both sent to and read from by a single
        // signal.  The rule is that the library must handle signal-level recursion itself more efficiently.
        //
        // We know that these cannot be a signal on both ends because signals can't recurse directly with other signals.
        let mut ignoring_edges = HashSet::<(NodeIndex, NodeIndex)>::new();

        for n in self.graph.node_indices() {
            for e in self.graph.edges_directed(n, Direction::Outgoing) {
                if self.graph.contains_edge(e.target(), n) {
                    ignoring_edges.insert((n, e.target()));
                    ignoring_edges.insert((e.target(), n));
                }
            }
        }
        self.graph.retain_edges(|g, e| {
            let edge = g.edge_endpoints(e).unwrap();
            !ignoring_edges.contains(&edge)
        });

        let recursive = petgraph::algo::is_cyclic_directed(&self.graph);

        Ok(TracedCharacteristics { recursive })
    }
}

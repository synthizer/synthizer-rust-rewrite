use std::borrow::Cow;

use ahash::{HashMap, HashMapExt};
use arrayvec::ArrayVec;

use crate::command::*;
use crate::config::*;
use crate::data_structures::{
    edgemap::{Edge, EdgeMap, GeneralizedEndpoint},
    stager::{Stager, StagerPolicy},
    AllocatedBlock, BlockAllocator,
};
use crate::nodes::{NodeAt, NodeDescriptor, NodeExecutionContext};
use crate::unique_id::UniqueId;
use crate::ChannelFormat;

/// Lives on the audio thread, and knows how to execute nodes and provide other services that nodes need to execute.
pub(crate) struct AudioGraph {
    /// By putting these in a struct, we get to avoid split borrows.
    executable_node_ctx: ExecutableNodeContext,

    nodes: HashMap<UniqueId, Box<dyn ExecutableNode>>,

    stager: Stager<UniqueId>,

    /// True if the graph's execution plan is still valid.
    plan_valid: bool,
}

pub(crate) struct ExecutableNodeContext {
    /// What is the output format of this graph?
    ///
    /// Each graph has one output format, which all audio is mixed to.
    pub output_format: ChannelFormat,

    /// Used by nodes in order to figure out their input and output buffers.
    edges: EdgeMap<Connection>,

    /// buffers which have been allocated for nodes by input.
    input_blocks: HashMap<InputRef, ArrayVec<AllocatedBlock, MAX_CHANNELS>>,
}

/// An output reference, the first half of an edge.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
struct OutputRef {
    node: UniqueId,
    output: u8,
}

/// An input reference, the second half of an edge.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
struct InputRef {
    node: UniqueId,
    input: u8,
}

#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
struct Connection {
    output: OutputRef,
    input: InputRef,
}

/// Object safe wrapper over nodes which lets us box the non-object-safe node trait.
pub(crate) trait ExecutableNode: Send + Sync + 'static {
    fn execute(
        &mut self,
        services: &mut crate::server::implementation::AudioThreadServerServices,
        params: &mut ExecutableNodeContext,
        output_buffers: &mut [AllocatedBlock],
    );

    fn get_descriptor(&self) -> &NodeDescriptor;

    /// Run a command.
    ///
    /// The default impl is unreachable.
    fn execute_command(&mut self, _cmd: &mut Command) {
        unreachable!();
    }
}

impl Edge for Connection {
    type Outgoing = OutputRef;
    type Incoming = InputRef;

    fn get_outgoing(&self) -> &Self::Outgoing {
        &self.output
    }

    fn get_incoming(&self) -> &Self::Incoming {
        &self.input
    }
}

impl GeneralizedEndpoint<UniqueId> for OutputRef {
    fn generalize(&self) -> &UniqueId {
        &self.node
    }
}

impl GeneralizedEndpoint<UniqueId> for InputRef {
    fn generalize(&self) -> &UniqueId {
        &self.node
    }
}

impl AudioGraph {
    pub(crate) fn new(
        output_format: ChannelFormat,
        expected_nodes: usize,
        expected_edges: usize,
    ) -> Self {
        AudioGraph {
            executable_node_ctx: ExecutableNodeContext {
                output_format,
                edges: EdgeMap::new(expected_edges),
                input_blocks: HashMap::with_capacity(expected_edges),
            },
            nodes: HashMap::with_capacity(expected_nodes),

            stager: Stager::new(expected_nodes + expected_edges, u16::MAX),
            plan_valid: false,
        }
    }

    /// Called on the audio thread. Executes the graph.
    ///
    /// Nodes will place whatever data they wish to output in the output buffers.  We model speakers as a specific node
    /// type.
    pub(crate) fn execute(
        &mut self,
        services: &mut crate::server::implementation::AudioThreadServerServices,
        output_blocks: &mut [AllocatedBlock],
    ) {
        assert_eq!(
            self.executable_node_ctx
                .output_format
                .get_channel_count()
                .get(),
            output_blocks.len()
        );

        struct Policy<'a>(
            &'a EdgeMap<Connection>,
            &'a HashMap<UniqueId, Box<dyn ExecutableNode>>,
        );

        impl<'a> StagerPolicy for Policy<'a> {
            type Node = UniqueId;

            fn determine_dependencies(
                &self,
                node: Self::Node,
                mut callback: impl FnMut(Self::Node),
            ) {
                self.0.iter_incoming(&node).for_each(|x| {
                    callback(x.output.node);
                });
            }

            fn determine_roots(&self, mut callback: impl FnMut(Self::Node)) {
                for n in self.1.keys() {
                    if self.0.iter_outgoing(n).count() == 0 {
                        callback(*n);
                    }
                }
            }
        }

        if !self.plan_valid {
            self.executable_node_ctx.edges.maintenance();
            self.stager.clear();
            self.stager
                .execute(&Policy(&self.executable_node_ctx.edges, &self.nodes));
        }

        for n in self.stager.iter() {
            self.nodes
                .get_mut(&n)
                .expect("Node should be in the map")
                .execute(services, &mut self.executable_node_ctx, output_blocks);
        }
    }

    /// Add a node to this graph.
    pub(crate) fn add_node(&mut self, id: UniqueId, node: Box<dyn ExecutableNode>) {
        let old = self.nodes.insert(id, node);
        assert!(
            old.is_none(),
            "Logic error: attempt to add the same node twice"
        );
    }

    /// Connect a node to another node.
    ///
    /// # Panics
    ///
    /// Validation of user input is done in the user-facing API which does not live on the audio thread.  If invalid
    /// input gets here, a panic results.
    pub(crate) fn connect(
        &mut self,
        outgoing_id: UniqueId,
        output: u8,
        incoming_id: UniqueId,
        input: u8,
    ) {
        let outgoing_node = self
            .nodes
            .get(&outgoing_id)
            .expect("Node should be in the graph");
        let _incoming_node = self
            .nodes
            .get(&incoming_id)
            .expect("Node should be in the graph");

        assert!((output as usize) < outgoing_node.get_descriptor().outputs.len());
        // We don't support inputs yet.
        assert!(input == 0);

        self.executable_node_ctx.edges.upsert(Connection {
            output: OutputRef {
                node: outgoing_id,
                output,
            },
            input: InputRef {
                node: incoming_id,
                input,
            },
        });
        self.plan_valid = false;
    }
}

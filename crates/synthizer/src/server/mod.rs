// We want to reserve this file specifically for the minimal public API of servers, so we put the shared implementation
// in impl and then put the handles here.
mod audio_output_thread;
pub(crate) mod implementation;

pub(crate) use audio_output_thread::*;
pub(crate) use implementation::ServerCommand;
use implementation::*;

use std::sync::{Arc, Mutex};

use audio_synchronization::concurrent_slab::ExclusiveSlabRef;

use crate::command::*;
use crate::data_structures::Graph;
use crate::error::*;
use crate::internal_object_handle::{InternalObjectHandle, ServerChannel};
use crate::nodes::traits::{NodeAt, NodeHandle, NodeHandleSealed};
use crate::nodes::*;
use crate::unique_id::UniqueId;
use crate::worker_pool::WorkerPoolHandle;

/// Part of a server which is behind Arc.
///
/// Lets the user clone without cloning a ton of arcs.
struct ServerInternal {
    server: ServerHandle,

    worker_pool: WorkerPoolHandle,

    /// If there is an audio thread, this is it.
    audio_thread: Option<AudioThread>,

    /// The server's graph.
    graph: Arc<Mutex<Graph>>,

    /// Id of the root (server) graph.
    ///
    /// For now, we only have one graph so this is just created in `fn new` so that there's something to put in handles.
    root_graph_id: UniqueId,
}

/// Represents an audio device.
///
/// All objects in Synthizer are associated with a server, and cross-server interactions are not supported.  Audio is
/// playing as long as the server is alive.
///
/// Contrary to the name, this does not involve networking.  It is borrowed terminology from other audio APIs
/// (supercollider, pyo, etc).
#[derive(Clone)]
pub struct Server {
    internal: Arc<ServerInternal>,
}

impl ServerInternal {
    pub fn new_default_device() -> Result<Self> {
        let worker_pool = WorkerPoolHandle::new(std::num::NonZeroUsize::new(1).unwrap(), false);
        let (server, callback) = ServerHandle::new(
            crate::channel_format::ChannelFormat::Stereo,
            Default::default(),
            worker_pool.clone(),
        );
        let audio_thread = AudioThread::new_with_default_device(callback)?;

        let h = ServerInternal {
            server,
            worker_pool,
            audio_thread: Some(audio_thread),
            graph: Arc::new(Mutex::new(Graph::new())),
            root_graph_id: UniqueId::new(),
        };

        Ok(h)
    }

    fn allocate<T: std::any::Any + Send + Sync>(&self, new_val: T) -> ExclusiveSlabRef<T> {
        self.server.allocate(new_val)
    }

    fn send_command(&self, command: Command) {
        self.server.send_command(command);
    }

    /// Mutate the graph behind the graph's mutex, then make sure the server picks that change up.
    fn mutate_graph(&self, graph_mutator: impl FnOnce(&mut Graph)) {
        let mut guard = self.graph.lock().unwrap();
        graph_mutator(&mut guard);
        self.send_command(Command::new(
            &Port::for_server(),
            ServerCommand::UpdateGraph {
                new_graph: guard.clone(),
            },
        ));
    }

    /// Register a node with the graph and the server.
    pub(crate) fn register_node(&self, id: UniqueId, handle: ConcreteNodeHandle) -> Result<()> {
        self.mutate_graph(|g| {
            g.register_node(id);
            // And while behind the graph's mutex, also ensure that the server knows about this node.
            self.send_command(Command::new(
                &Port::for_server(),
                ServerCommand::RegisterNode {
                    id,
                    descriptor: handle.describe_erased(),
                    handle,
                },
            ));
        });
        Ok(())
    }

    /// Connect the nth output of a given node to the nth input of another given node.
    ///
    /// For now both values must be in range and are unvalidated; validation at the type level is pending.
    fn connect<O: NodeHandle, I: NodeHandle>(
        &self,
        output_node: &O,
        output_index: usize,
        input_node: &I,
        input_index: usize,
    ) -> Result<()> {
        self.mutate_graph(|g| {
            g.connect(
                output_node.get_id(),
                output_index,
                input_node.get_id(),
                input_index,
            );
        });
        Ok(())
    }
}

impl ServerChannel for ServerInternal {
    fn deregister_object(&self, id: UniqueId) {
        self.mutate_graph(|g| {
            g.deregister_node(id);
        });

        self.server.send_command(Command::new(
            &Port::for_server(),
            ServerCommand::DeregisterObject { id },
        ));
    }

    fn send_command(&self, command: Command) -> Result<()> {
        self.server.send_command(command);
        Ok(())
    }
}

impl Server {
    pub fn new_default_device() -> Result<Self> {
        Ok(Self {
            internal: Arc::new(ServerInternal::new_default_device()?),
        })
    }

    pub(crate) fn allocate<T: std::any::Any + Send + Sync>(
        &self,
        new_val: T,
    ) -> ExclusiveSlabRef<T> {
        self.internal.allocate(new_val)
    }

    pub(crate) fn register_node(
        &self,
        id: UniqueId,
        handle: ConcreteNodeHandle,
    ) -> Result<InternalObjectHandle> {
        self.internal.register_node(id, handle)?;

        let graph_id = self.internal.root_graph_id;
        Ok(InternalObjectHandle {
            server_chan: self.internal.clone(),
            object_id: id,
            graph_id,
        })
    }

    pub(crate) fn worker_pool(&self) -> &WorkerPoolHandle {
        &self.internal.worker_pool
    }

    /// Connect the nth output of a given node to the nth input of another given node.
    ///
    /// For now both values must be in range and are unvalidated; validation at the type level is pending.
    pub fn connect<O: NodeHandle, I: NodeHandle>(
        &self,
        output_node: &O,
        output_index: usize,
        input_node: &I,
        input_index: usize,
    ) -> Result<()> {
        self.internal
            .connect(output_node, output_index, input_node, input_index)
    }
}

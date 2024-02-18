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
use crate::nodes::traits::NodeHandle;
use crate::nodes::*;
use crate::unique_id::UniqueId;
use crate::worker_pool::WorkerPoolHandle;

/// Servers are either sending audio to a device or waiting on the user to get it by hand.
enum ServerDestination {
    AudioDevice(AudioThread),
    Inline(Mutex<ServerExecutionCallback>),
}

/// Part of a server which is behind Arc.
///
/// Lets the user clone without cloning a ton of arcs.
struct ServerInternal {
    server: ServerHandle,

    worker_pool: WorkerPoolHandle,

    destination: ServerDestination,

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
///
/// In addition to pushing data to an audio device, it is possible to pull data to your own code as f32 samples at the
/// server's internal sampling rate, [Server::get_sr].  if resampling is required, you must currently handle that
/// yourself.
#[derive(Clone)]
pub struct Server {
    internal: Arc<ServerInternal>,
}

impl ServerInternal {
    /// Part of new common to all servers.
    fn new_common(
        server: ServerHandle,
        worker_pool: WorkerPoolHandle,
        destination: ServerDestination,
    ) -> Result<Self> {
        let h = ServerInternal {
            server,
            worker_pool,
            destination,
            graph: Arc::new(Mutex::new(Graph::new())),
            root_graph_id: UniqueId::new(),
        };

        Ok(h)
    }

    fn new_default_device() -> Result<Self> {
        let worker_pool = WorkerPoolHandle::new_threaded(std::num::NonZeroUsize::new(1).unwrap());
        let (server, callback) = ServerHandle::new(
            crate::channel_format::ChannelFormat::Stereo,
            Default::default(),
            worker_pool.clone(),
        );
        let audio_thread = AudioThread::new_with_default_device(callback)?;
        let destination = ServerDestination::AudioDevice(audio_thread);
        Self::new_common(server, worker_pool, destination)
    }

    pub fn new_inline() -> Result<Self> {
        let worker_pool = WorkerPoolHandle::new_inline();
        let (server, callback) = ServerHandle::new(
            crate::channel_format::ChannelFormat::Stereo,
            Default::default(),
            worker_pool.clone(),
        );

        let destination = ServerDestination::Inline(Mutex::new(callback));
        Self::new_common(server, worker_pool, destination)
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

    /// Synthesize some data.  Currently corresponds to the public `synthesize_stereo` method and requires the channel
    /// format be fixed to stereo to match server implementation assumptions.
    ///
    /// Panics if this is called from more than one thread simultaneously; errors if the server is actually for audio
    /// output.
    fn synthesize_data(&self, format: crate::ChannelFormat, destination: &mut [f32]) -> Result<()> {
        assert_eq!(format, crate::ChannelFormat::Stereo);

        if destination.len() % format.get_channel_count() != 0 {
            return Err(Error::new_validation_cow(format!(
                "Got a slice of length {} which must be a multiple of the frame size {}",
                destination.len(),
                format.get_channel_count()
            )));
        }

        match &self.destination {
            ServerDestination::AudioDevice(_) => {
                return Err(Error::new_validation_cow(
                    "This server is for audio devices, not inline synthesis",
                ));
            }
            ServerDestination::Inline(cb) => {
                let mut cb = cb
                    .try_lock()
                    .expect("Server synthesis should only ever happen on one thread");
                (*cb)(destination);
            }
        }

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

    /// Create a server which is intended to be run to retrieve samples from Synthizer.
    ///
    /// This server may have data pulled from it with [Server::get_block].
    pub fn new_inline() -> Result<Self> {
        Ok(Self {
            internal: Arc::new(ServerInternal::new_inline()?),
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

    /// Get the sampling rate of this server.
    ///
    /// This is the sampling rate which will be used when calling [Server::synthesize_data].
    pub fn get_sr(&self) -> u64 {
        crate::config::SR as u64
    }

    /// get Synthizer to synthesize a block of stereo data.
    ///
    /// This function will perform synthesis on the current thread, writing to the given output slice in the server's
    /// sampling rate.  Note that this includes things such as running streaming sources.
    ///
    /// If this function is called from multiple threads, a panic results.  Since Synthizer has chosen to provide
    /// Arc-like handles, this can't be defended against at the type level.
    ///
    /// # Panics
    ///
    /// If this is called from more than one thread simultaneously for the same server, even if those calls come from different handles.
    pub fn synthesize_stereo(&self, destination: &mut [f32]) -> Result<()> {
        self.internal
            .synthesize_data(crate::ChannelFormat::Stereo, destination)
    }
}

//! Infrastructure for object handles.
//!
//! These handles are wrapped by concrete named structs, and are how objects internally communicate to audio threads.
use std::sync::Arc;

use crate::command::Command;
use crate::error::Result;
use crate::unique_id::UniqueId;

/// A trait representing the ability to send commands, and perform other abstracted server operations needed for
/// handles.
///
/// Synthizer provides multiple kinds of server, namely one for single-threaded synthesis where the user wants samples
/// and one for output to an audio device. What these have in common is that they contain some way to send commands
/// across.  This trait abstracts over the idea of a server, so that references to it need not concern themselves with
/// which kind it is.
pub(crate) trait ServerChannel: Send + Sync + 'static {
    /// Send a command to a destination.
    ///
    /// This may choose to fail, e.g. if the user requests that backpressure be reported as errors rather than blocking.
    fn send_command(&self, command: Command) -> Result<()>;

    /// Deregister an object.
    ///
    /// This must always succeed, as it is called in a drop impl.
    fn deregister_object(&self, id: UniqueId);
}

/// An internal handle to an object, recording how an object may communicate with a server and associated ids.
///
/// Only objects which can be mutated/manipulated on audio threads get handles.  For example, all nodes have a handle,
/// but things like audio buffers which don't even belong to a server don't.  Servers themselves also dont' get handles
/// via this mechanism, since they are a key top-level object.  The user is given an opaque server handle which itself
/// erases the kind of server it is, but this is done by hand.
///
/// One of the core rules of Synthizer code is that all mutation happens via commands on the audio thread, and so this
/// handle serves as a way to get commands to the audio thread.  Once an object lives on the audio thread, no other
/// thread has the power to access that object directly.
///
/// Internally handles themselves do not clone, but handles are exposed to the user implicitly behind [Arc] nested
/// inside specific, named handle types so that the handles we give to users may clone.
///
/// Handles are created by [crate::server::ServerHandle] and, when we get around to implementing subgraphs, that will be
/// handled by a special case which changes the graph id.
pub(crate) struct InternalObjectHandle {
    pub(crate) server_chan: Arc<dyn ServerChannel>,

    pub(crate) object_id: UniqueId,

    /// This is the id of the graph.  Two handles may only connect to one another if in the same graph.
    pub(crate) graph_id: UniqueId,
}

impl Drop for InternalObjectHandle {
    fn drop(&mut self) {
        self.server_chan.deregister_object(self.object_id);
    }
}

impl InternalObjectHandle {
    pub(crate) fn send_command(&self, command: impl Into<Command>) -> Result<()> {
        self.server_chan.send_command(command.into())
    }

    pub(crate) fn is_same_graph(&self, other: &InternalObjectHandle) -> bool {
        self.graph_id == other.graph_id
    }
}

impl crate::command::CommandSender for InternalObjectHandle {
    fn send_impl(&self, command: Command) -> Result<()> {
        self.server_chan.send_command(command)
    }
}

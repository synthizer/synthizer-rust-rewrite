// We want to reserve this file specifically for the minimal public API of servers, so we put the shared implementation
// in impl and then put the handles here.

mod audio_output_server;

pub(crate) mod implementation;

pub(crate) use audio_output_server::*;
pub(crate) use implementation::ServerCommand;

use std::sync::Arc;

use audio_synchronization::concurrent_slab::ExclusiveSlabRef;

use crate::command::*;
use crate::error::*;
use crate::unique_id::UniqueId;

#[derive(derive_more::IsVariant)]
enum ServerKind {
    AudioOutput(AudioOutputServer),
}

impl ServerKind {
    fn send_command(&self, command: Command) {
        match self {
            ServerKind::AudioOutput(ref s) => s.send_command(command),
        }
    }

    fn allocate<T: std::any::Any + Send + Sync>(&self, new_val: T) -> ExclusiveSlabRef<T> {
        match self {
            ServerKind::AudioOutput(x) => x.allocate(new_val),
        }
    }
}

/// A handle representing an audio device.
///
/// All objects in Synthizer are associated with a server, and cross-server interactions are not supported.  Audio is
/// playing as long as the server is alive.
///
/// Contrary to the name, this does not involve networking.  It is borrowed terminology from other audio APIs
/// (supercollider, pyo, etc).
#[derive(Clone)]
pub struct ServerHandle {
    kind: Arc<ServerKind>,
}

impl ServerHandle {
    pub fn new_default_device() -> Result<Self> {
        let backend = AudioOutputServer::new_with_default_device()?;
        let kind = ServerKind::AudioOutput(backend);
        Ok(ServerHandle {
            kind: Arc::new(kind),
        })
    }

    fn send_command(&self, command: Command) {
        self.kind.send_command(command);
    }

    /// This is temporary: Start a sine wave of a given frequency, running forever.
    pub fn start_sin(&self, freq: f64) -> Result<()> {
        let node = self
            .kind
            .allocate(crate::nodes::trig::TrigWaveform::new_sin(freq));
        let cmd = Command::new(
            &Port::for_server(),
            ServerCommand::RegisterNode {
                id: UniqueId::new(),
                handle: node.into(),
            },
        );
        self.send_command(cmd);
        Ok(())
    }
}

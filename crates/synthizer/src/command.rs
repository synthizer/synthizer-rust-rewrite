use std::any::Any;

use arrayvec::ArrayVec;

use crate::error::Result;
use crate::unique_id::UniqueId;

/// A port is returned on object creation and tells commands where they are going.  This is what non-audio-thread
/// objects get and use to dispatch against a server.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct Port {
    pub(crate) kind: PortKind,
}

#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Hash, derive_more::IsVariant, Debug)]
pub(crate) enum PortKind {
    /// Servers cannot be objects, since they own objects. We special case that instead.
    Server,

    Node(UniqueId),
}

impl Port {
    /// Return a port for a server.
    pub(crate) fn for_server() -> Self {
        Port {
            kind: PortKind::Server,
        }
    }

    /// Return a port for a node.
    pub(crate) fn for_node(node: UniqueId) -> Port {
        Port {
            kind: PortKind::Node(node),
        }
    }
}

impl AsRef<Port> for Port {
    fn as_ref(&self) -> &Port {
        self
    }
}

/// We have to isolate imports because the macro is using magic to allow for `paste` usage.
mod cmdkind {
    use crate::properties::PropertyCommand;
    use crate::server::implementation::ServerCommand;

    use super::*;
    use crate::common_commands::*;

    variant!(pub(crate) CommandKind, ServerCommand, PropertyCommand, SetLoopConfigCommand);
}
pub(crate) use cmdkind::*;

#[derive(Debug)]
pub(crate) struct Command {
    port: Port,
    payload: CommandKind,
}

impl Command {
    pub(crate) fn port(&self) -> &Port {
        &self.port
    }

    pub(crate) fn new(port: &Port, what: impl Into<CommandKind>) -> Self {
        Self {
            port: *port,
            payload: what.into(),
        }
    }

    pub(crate) fn extract_payload_as<T: Any>(self) -> Result<T, Self>
    where
        T: TryFrom<CommandKind, Error = CommandKind>,
    {
        self.payload
            .try_into()
            .map_err(|payload| Self { payload, ..self })
    }

    pub(crate) fn take_call<T: CommandKindPayload>(
        self,
        closure: impl FnOnce(T),
    ) -> Result<(), Self> {
        self.payload.take_call(closure).map_err(|payload| Command {
            port: self.port,
            payload,
        })
    }
}

/// Something that knows how to dispatch commands.
pub(crate) trait CommandSender {
    fn send_impl(&self, command: Command) -> Result<()>;
}

/// Convenient methods over all [CommandSender]s which lets that trait be object safe.
pub(crate) trait CommandSenderExt {
    /// Convenience method to prevent having to use Command::new everywhere.
    fn send<C, P>(&self, port: P, payload: C) -> Result<()>
    where
        CommandKind: From<C>,
        P: AsRef<Port>;
}

impl<T: CommandSender + ?Sized> CommandSenderExt for T {
    fn send<C, P>(&self, port: P, payload: C) -> Result<()>
    where
        CommandKind: From<C>,
        P: AsRef<Port>,
    {
        self.send_impl(Command::new(port.as_ref(), payload))
    }
}

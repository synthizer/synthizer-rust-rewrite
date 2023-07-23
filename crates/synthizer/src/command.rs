use std::any::Any;

use arrayvec::ArrayVec;

use crate::unique_id::UniqueId;

/// A port is returned on object creation and tells commands where they are going.  This is what non-audio-thread
/// objects get and use to dispatch against a server.
#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct Port {
    pub(crate) kind: PortKind,
}

#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Hash, derive_more::IsVariant)]
pub(crate) enum PortKind {
    /// Servers cannot be objects, since they own objects. We special case that instead.
    Server,

    Node(UniqueId),
}

pub(crate) trait CommandPayload<E> {
    fn to_command(self) -> E;
}

impl Port {
    /// Return a port for a server.
    pub(crate) fn for_server() -> Self {
        Port {
            kind: PortKind::Server,
        }
    }

    /// Return a port for a node.
    fn for_node(node: UniqueId) -> Port {
        Port {
            kind: PortKind::Node(node),
        }
    }
}

/// We have to isolate imports because the macro is using magic to allow for `paste` usage.
mod cmdkind {
    use crate::graph::GraphCommand;
    use crate::server::implementation::ServerCommand;

    use super::*;

    variant!(pub(crate) CommandKind, ServerCommand);
}
use cmdkind::*;

pub(crate) struct Command {
    port: Port,
    payload: CommandKind,
}

impl Command {
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
}

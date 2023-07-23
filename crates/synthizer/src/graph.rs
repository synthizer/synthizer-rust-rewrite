use crate::data_structures::audio_graph::ExecutableNode;
use crate::unique_id::UniqueId;

pub(crate) enum GraphCommand {
    CreateNode {
        id: UniqueId,
        /// Moved out by the raph.
        node: Option<Box<dyn ExecutableNode>>,
    },

    Connect {
        outgoing_node: UniqueId,
        output: u8,
        incoming_node: UniqueId,
        input: u8,
    },
}

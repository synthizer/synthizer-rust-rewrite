use std::borrow::Cow;

use crate::channel_format::ChannelFormat;

use super::descriptor::*;
use super::traits::*;
use super::*;

pub(crate) struct Inputs<'a> {
    input: &'a [AllocatedBlock],
}

impl<'a> ToNamedInputs<'a> for Inputs<'a> {
    fn to_named_inputs<'b>(inputs: &'b mut InputsByIndex<'a>) -> Self {
        Inputs { input: inputs[0] }
    }
}

/// A node which copies its input to the audio device's output.
///
/// The input format of this node is user-specified. The output format of this node matches the server.  This is useful
/// because an application that wishes to output stereo which happens to be connected to a 5.1 speaker setup can save a
/// lot of remixing work by remixing at the input of this node, then let this node do the final remix to the output.
pub(crate) struct AudioOutputNode {
    format: ChannelFormat,
}

impl HasNodeDescriptor for AudioOutputNode {
    type Inputs<'a> = Inputs<'a>;
    type Outputs<'a> = ();

    fn describe(&self) -> Cow<'static, NodeDescriptor> {
        return Cow::Owned(NodeDescriptor {
            inputs: Cow::Owned(vec![InputDescriptor {
                channel_format: self.format.clone(),
            }]),
            outputs: Cow::Borrowed(&[]),
        });
    }
}

impl NodeAt for AudioOutputNode {
    fn execute<'a>(
        &'a mut self,
        context: &'a mut NodeExecutionContext<Self>,
    ) -> NodeExecutionOutcome {
        // All this node does is remixes the input to the output.
        crate::channel_conversion::convert_channels(
            &self.format,
            context.speaker_format,
            context.inputs.input,
            context.speaker_outputs,
            // The server will zero this before this node runs.
            true,
        );
        NodeExecutionOutcome::SentAudio
    }
}

impl AudioOutputNode {
    pub(crate) fn new(format: ChannelFormat) -> AudioOutputNode {
        AudioOutputNode { format }
    }
}

use std::borrow::Cow;
use std::sync::Arc;

use crate::config::BLOCK_SIZE;
use crate::error::Result;
use crate::internal_object_handle::InternalObjectHandle;
use crate::math::trig_waveforms::TrigWaveformEvaluator;
use crate::nodes::*;
use crate::server::ServerHandle;
use crate::unique_id::UniqueId;

/// Kinds of trigonometric waveform to use with [TrigWaveform].
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum TrigWaveformKind {
    Sin,
    Cos,
    Tan,
}

/// A node representing a trigonometric waveform.
pub(crate) struct TrigWaveformNode {
    evaluator: TrigWaveformEvaluator,
}

pub(crate) struct TrigWaveformOutputs<'a> {
    output: OutputDestination<'a>,
}

// TODO: macro this.
impl<'a> ToNamedOutputs<'a> for TrigWaveformOutputs<'a> {
    fn to_named_outputs<'b>(
        outputs: &'b mut crate::nodes::OutputsByIndex<'a>,
    ) -> TrigWaveformOutputs<'a> {
        TrigWaveformOutputs {
            output: outputs.pop_at(0).unwrap(),
        }
    }
}

impl HasNodeDescriptor for TrigWaveformNode {
    type Outputs<'a> = TrigWaveformOutputs<'a>;
    type Inputs<'a> = ();

    fn describe(&self) -> Cow<'static, NodeDescriptor> {
        use crate::channel_format::ChannelFormat;
        use crate::nodes::*;

        Cow::Borrowed(&NodeDescriptor {
            outputs: Cow::Borrowed(&[OutputDescriptor {
                channel_format: ChannelFormat::Mono,
            }]),
            inputs: Cow::Borrowed(&[]),
        })
    }
}

impl NodeAt for TrigWaveformNode {
    fn execute(
        &mut self,
        context: &mut crate::nodes::NodeExecutionContext<Self>,
    ) -> crate::nodes::NodeExecutionOutcome {
        use crate::nodes::OutputDestination as OD;
        match &mut context.outputs.output {
            OD::Block(s) => {
                self.evaluator
                    .evaluate_ticks(BLOCK_SIZE, |i, v| s[0].write(i, v));
                crate::nodes::NodeExecutionOutcome::SentAudio
            }
        }
    }
}

impl TrigWaveformNode {
    pub(crate) fn new_sin(freq: f64) -> Self {
        TrigWaveformNode {
            evaluator: TrigWaveformEvaluator::new_sin(freq, 0.0),
        }
    }
}

#[derive(Clone)]
pub struct TrigWaveformNodeHandle {
    internal_handle: Arc<InternalObjectHandle>,
}

impl TrigWaveformNodeHandle {
    pub fn new_sin(server: &ServerHandle, frequency: f64) -> Result<TrigWaveformNodeHandle> {
        let internal_handle = Arc::new(server.register_node(
            UniqueId::new(),
            server.allocate(TrigWaveformNode::new_sin(frequency)).into(),
        )?);
        Ok(TrigWaveformNodeHandle { internal_handle })
    }
}

impl super::NodeHandleSealed for TrigWaveformNodeHandle {
    fn get_id(&self) -> UniqueId {
        self.internal_handle.object_id
    }
}
impl super::NodeHandle for TrigWaveformNodeHandle {}

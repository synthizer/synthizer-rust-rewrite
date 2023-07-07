use std::borrow::Cow;

use crate::config::BLOCK_SIZE;
use crate::math::trig_waveforms::TrigWaveformEvaluator;
use crate::node::{FromOutputSlice, Node, OutputDestination};
use crate::node_descriptor::NodeDescriptor;

/// Kinds of trigonometric waveform to use with [TrigWaveform].
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum TrigWaveformKind {
    Sin,
    Cos,
    Tan,
}

/// A node representing a trigonometric waveform.
pub struct TrigWaveform;

pub(crate) struct TrigWaveformState {
    evaluator: TrigWaveformEvaluator,
}

pub(crate) struct TrigWaveformOutputs<'a> {
    output: &'a mut OutputDestination<'a>,
}

// TODO: macro this.
impl<'a> FromOutputSlice for TrigWaveformOutputs<'a> {
    type NamedOutputs<'b> = TrigWaveformOutputs<'b>;

    fn to_named_outputs(mut outputs: crate::node::OutputsByIndex) -> TrigWaveformOutputs {
        TrigWaveformOutputs {
            output: outputs.pop_at(0).unwrap(),
        }
    }
}

impl Node for TrigWaveform {
    type Outputs<'a> = TrigWaveformOutputs<'a>;
    type State = TrigWaveformState;

    fn describe(_state: &Self::State) -> Cow<'static, crate::node_descriptor::NodeDescriptor> {
        use crate::channel_format::ChannelFormat;
        use crate::node_descriptor::*;

        Cow::Borrowed(&NodeDescriptor {
            outputs: Cow::Borrowed(&[OutputDescriptor {
                channel_format: ChannelFormat::Mono,
            }]),
        })
    }

    fn execute(
        context: &mut crate::node::NodeExecutionContext<Self>,
    ) -> crate::node::NodeExecutionOutcome {
        use crate::node::OutputDestination as OD;
        match context.outputs.output {
            OD::Block(s) => {
                context
                    .state
                    .evaluator
                    .evaluate_ticks(BLOCK_SIZE, |i, v| s[0].write(i, v));
                crate::node::NodeExecutionOutcome::SentAudio
            }
        }
    }
}

use std::borrow::Cow;

use crate::config::BLOCK_SIZE;
use crate::math::trig_waveforms::TrigWaveformEvaluator;
use crate::nodes::*;

/// Kinds of trigonometric waveform to use with [TrigWaveform].
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum TrigWaveformKind {
    Sin,
    Cos,
    Tan,
}

/// A node representing a trigonometric waveform.
pub struct TrigWaveform {
    evaluator: TrigWaveformEvaluator,
}

pub(crate) struct TrigWaveformOutputs<'a> {
    output: OutputDestination<'a>,
}

// TODO: macro this.
impl<'a> FromOutputSlice<'a> for TrigWaveformOutputs<'a> {
    fn to_named_outputs<'b>(
        outputs: &'b mut crate::nodes::OutputsByIndex<'a>,
    ) -> TrigWaveformOutputs<'a> {
        TrigWaveformOutputs {
            output: outputs.pop_at(0).unwrap(),
        }
    }
}

impl NodeAt for TrigWaveform {
    type Outputs<'a> = TrigWaveformOutputs<'a>;

    fn describe(&self) -> Cow<'static, NodeDescriptor> {
        use crate::channel_format::ChannelFormat;
        use crate::nodes::*;

        Cow::Borrowed(&NodeDescriptor {
            outputs: Cow::Borrowed(&[OutputDescriptor {
                channel_format: ChannelFormat::Mono,
            }]),
        })
    }

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

impl TrigWaveform {
    pub(crate) fn new_sin(freq: f64) -> Self {
        TrigWaveform {
            evaluator: TrigWaveformEvaluator::new_sin(freq, 0.0),
        }
    }
}

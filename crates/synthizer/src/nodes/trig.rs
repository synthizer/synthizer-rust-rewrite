use std::borrow::Cow;
use std::sync::Arc;

use crate::command::Port;
use crate::config::BLOCK_SIZE;
use crate::error::Result;
use crate::internal_object_handle::InternalObjectHandle;
use crate::math::trig_waveforms::TrigWaveformEvaluator;
use crate::nodes::*;
use crate::properties::*;
use crate::server::Server;
use crate::unique_id::UniqueId;

/// Kinds of trigonometric waveform to use with [TrigWaveform].
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum TrigWaveformKind {
    Sin,
    Cos,
    Tan,
}

pub(crate) struct TrigWaveformNodeAt {
    evaluator: TrigWaveformEvaluator,
    props: TrigWaveformSlots,
}

#[synthizer_macros_internal::property_slots]
pub struct TrigWaveformSlots {
    pub(super) frequency: Slot<F64X1>,
}

#[derive(synthizer_macros_internal::ToNamedOutputs)]
pub(crate) struct TrigWaveformOutputs<'a> {
    output: OutputDestination<'a>,
}

impl HasNodeDescriptor for TrigWaveformNodeAt {
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

impl NodeAt for TrigWaveformNodeAt {
    type Properties = TrigWaveformSlots;

    fn get_property_struct(&mut self) -> &mut Self::Properties {
        &mut self.props
    }

    fn execute(
        &mut self,
        context: &mut crate::nodes::NodeExecutionContext<Self>,
    ) -> crate::nodes::NodeExecutionOutcome {
        use crate::nodes::OutputDestination as OD;

        if let Some(f) = self.props.frequency.get_value_if_changed() {
            self.evaluator.set_frequency(f);
        }
        match &mut context.outputs.output {
            OD::Block(s) => {
                self.evaluator
                    .evaluate_ticks(BLOCK_SIZE, |i, v| s[0].write(i, v));
                crate::nodes::NodeExecutionOutcome::SentAudio
            }
        }
    }
}

impl TrigWaveformNodeAt {
    pub(crate) fn new_sin(freq: f64) -> Self {
        TrigWaveformNodeAt {
            evaluator: TrigWaveformEvaluator::new_sin(freq, 0.0),
            props: TrigWaveformSlots {
                frequency: Slot::new(freq),
            },
        }
    }
}

/// A node representing a trigonometric waveform.
#[derive(Clone)]
pub struct TrigWaveformNode {
    internal_handle: Arc<InternalObjectHandle>,
}

impl TrigWaveformNode {
    pub fn new_sin(server: &Server, frequency: f64) -> Result<TrigWaveformNode> {
        let internal_handle = Arc::new(
            server.register_node(
                UniqueId::new(),
                server
                    .allocate(TrigWaveformNodeAt::new_sin(frequency))
                    .into(),
            )?,
        );
        Ok(TrigWaveformNode { internal_handle })
    }

    pub fn props(&self) -> TrigWaveformProps {
        TrigWaveformProps::new(
            &*self.internal_handle,
            Port::for_node(self.internal_handle.object_id),
        )
    }
}

impl super::NodeHandleSealed for TrigWaveformNode {
    fn get_id(&self) -> UniqueId {
        self.internal_handle.object_id
    }
}

impl super::NodeHandle for TrigWaveformNode {}

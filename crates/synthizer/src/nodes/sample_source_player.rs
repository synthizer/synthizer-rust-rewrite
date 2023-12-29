use std::borrow::Cow;
use std::sync::Arc;

use crate::command::{CommandSender, Port};
use crate::config::BLOCK_SIZE;
use crate::error::Result;
use crate::internal_object_handle::InternalObjectHandle;
use crate::math::trig_waveforms::TrigWaveformEvaluator;
use crate::nodes::*;
use crate::properties::*;
use crate::sample_sources::{reader::SampleSourceReader, SampleSource};
use crate::server::Server;
use crate::unique_id::UniqueId;

pub(crate) struct SampleSourcePlayerNodeAt {
    executor: SampleSourceReader,
    props: (),
}

#[derive(synthizer_macros_internal::ToNamedOutputs)]
pub(crate) struct SampleSourcePlayerOutputs<'a> {
    output: OutputDestination<'a>,
}

impl HasNodeDescriptor for SampleSourcePlayerNodeAt {
    type Outputs<'a> = SampleSourcePlayerOutputs<'a>;
    type Inputs<'a> = ();

    fn describe(&self) -> Cow<'static, NodeDescriptor> {
        use crate::channel_format::ChannelFormat;
        use crate::nodes::*;

        let channel_format: ChannelFormat = self.executor.descriptor().channel_format.clone();

        Cow::Owned(NodeDescriptor {
            outputs: Cow::Owned(vec![OutputDescriptor { channel_format }]),
            inputs: Cow::Borrowed(&[]),
        })
    }
}

impl NodeAt for SampleSourcePlayerNodeAt {
    type Properties = ();

    fn get_property_struct(&mut self) -> &mut Self::Properties {
        &mut self.props
    }

    fn execute(
        &mut self,
        context: &mut crate::nodes::NodeExecutionContext<Self>,
    ) -> crate::nodes::NodeExecutionOutcome {
        use super::OutputDestination as OD;

        const BUFSIZE: usize = BLOCK_SIZE * MAX_CHANNELS;

        // We assume the executor handles resampling, and the node is already using the same channel format. To this
        // end, fill a thread-local buffer which is large enough for one block, then un-interleave it to the
        // destination.

        thread_local! {
            static BUFFER: std::cell::RefCell<[f32;BUFSIZE]> = const { std::cell::RefCell::new(
                [0.0f32; BUFSIZE]
            )
            };
        }

        let chans = self
            .executor
            .descriptor()
            .channel_format
            .get_channel_count()
            .get();
        BUFFER.with(|tmp_refcell| {
            let mut tmp = tmp_refcell.borrow_mut();
            let dest_slice = &mut tmp[..BLOCK_SIZE * chans];
            let frames_done = self.executor.read_samples(dest_slice).unwrap_or(0) as usize;

            match &mut context.outputs.output {
                OD::Block(o) => {
                    for ch in 0..chans {
                        let cur_block = &mut o[ch];

                        for i in 0..frames_done {
                            let index = ch + chans * i;
                            let sample = tmp[index];
                            cur_block.write(i, sample as f64);
                        }
                    }
                }
            }
        });

        NodeExecutionOutcome::SentAudio
    }
}

impl SampleSourcePlayerNodeAt {
    fn new(executor: SampleSourceReader) -> Self {
        Self {
            executor,
            props: (),
        }
    }
}

/// A node representing a [SampleSource].
#[derive(Clone)]
pub struct SampleSourcePlayerNode {
    internal_handle: Arc<InternalObjectHandle>,
}

impl SampleSourcePlayerNode {
    pub fn new<S: SampleSource>(server: &Server, source: S) -> Result<Self> {
        let id = UniqueId::new();
        let executor = SampleSourceReader::new(Box::new(source))?;

        let at = SampleSourcePlayerNodeAt::new(executor);

        let internal_handle = Arc::new(server.register_node(id, server.allocate(at).into())?);
        Ok(Self { internal_handle })
    }
}

impl super::NodeHandleSealed for SampleSourcePlayerNode {
    fn get_id(&self) -> UniqueId {
        self.internal_handle.object_id
    }
}

impl super::NodeHandle for SampleSourcePlayerNode {}

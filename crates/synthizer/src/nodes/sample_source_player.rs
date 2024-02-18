use crate::common_commands::*;
use crate::config::BLOCK_SIZE;
use crate::error::Result;
use crate::internal_object_handle::InternalObjectHandle;

use crate::nodes::*;

use crate::sample_sources::{execution::Executor, Descriptor as SDescriptor, SampleSource};
use crate::server::Server;
use crate::unique_id::UniqueId;
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

pub(crate) struct SampleSourcePlayerNodeAt {
    executor: Executor,
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
            let frames_done = self.executor.read_block(dest_slice).unwrap_or(0) as usize;

            // Note that the implementation gives us already uninterleaved blocks.
            match &mut context.outputs.output {
                OD::Block(o) => {
                    for ch in 0..chans {
                        let cur_block = &mut o[ch];

                        for i in 0..frames_done {
                            let index = ch * BLOCK_SIZE + i;
                            let sample = tmp[index];
                            cur_block.write(i, sample as f64);
                        }
                    }
                }
            }
        });

        NodeExecutionOutcome::SentAudio
    }

    fn execute_command(
        &mut self,
        cmd: crate::command::Command,
    ) -> std::prelude::v1::Result<(), crate::command::Command> {
        cmd.take_call(|cmd: SetLoopConfigCommand| {
            self.executor.config_looping(cmd.0);
        })
        .or_else(|x| {
            x.take_call::<SeekCommand>(|seek| {
                let _ = self.executor.seek(seek.0);
                // We can't log just yet. We need to do a small logging framework first.
            })
        })
    }
}

impl SampleSourcePlayerNodeAt {
    fn new(executor: Executor) -> Self {
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
    descriptor: SDescriptor,
}

impl SampleSourcePlayerNode {
    pub fn new<S: SampleSource>(server: &Server, source: S) -> Result<Self> {
        let id = UniqueId::new();
        let worker_pool = server.worker_pool();
        let executor = Executor::new(worker_pool, source)?;
        let descriptor = executor.descriptor().clone();

        if descriptor.duration == Some(0) {
            return Err(crate::error::Error::new_validation_static(
                "It is not possible to create sources whose duration is 0",
            ));
        }

        let at = SampleSourcePlayerNodeAt::new(executor);

        let internal_handle = Arc::new(server.register_node(id, server.allocate(at).into())?);
        Ok(Self {
            internal_handle,
            descriptor,
        })
    }

    /// Configure this node to loop.
    ///
    /// See node-level documentation for specific behaviors and guarantees as to what does and doesn't work,
    /// particularly as regards streams with unknown duration and streams which cannot seek accurately.
    pub fn config_looping(&self, specification: crate::LoopSpec) -> Result<()> {
        specification.validate(self.descriptor.sample_rate.get(), self.descriptor.duration)?;

        self.internal_handle
            .send_command_node(SetLoopConfigCommand(specification))?;
        Ok(())
    }

    /// Seek to a given position in the underlying source given as a sample in the sampling rate of the source.
    ///
    /// This desugars to a direct seek call on the source.  Note that this function returning `Ok` doesn't mean the seek
    /// went through, only that Synthizer believes that the seek is to a valid position.  The actual seek happens later
    /// on another thread.
    pub fn seek_sample(&self, new_pos: u64) -> Result<()> {
        use crate::sample_sources::SeekSupport;

        // We validate here because this is the only place that happens on a thread controlled by the user, so we
        // unfortunately can't abstract this into our internal module much.
        match self.descriptor.seek_support {
            SeekSupport::None => {
                return Err(crate::Error::new_validation_static(
                    "Seeking is not supported for this source",
                ))
            }
            SeekSupport::ToBeginning => {
                if new_pos != 0 {
                    return Err(crate::Error::new_validation_static(
                        "This source only supports seeking to time 0",
                    ));
                }
            }
            SeekSupport::Imprecise | SeekSupport::SampleAccurate => {
                if let Some(max) = self.descriptor.duration {
                    if new_pos >= max {
                        return Err(crate::Error::new_validation_static(
                            "Attempt to seek past the end of this source",
                        ));
                    }
                }
            }
        };

        self.internal_handle
            .send_command_node(SeekCommand(new_pos))?;
        Ok(())
    }

    /// Convenience function to seek to a given duration in seconds.
    ///
    /// This function works by converting the duration passed in to samples, then calling [Self::seek_sample] for you.
    pub fn seek(&self, dur: Duration) -> Result<()> {
        let secs = dur.as_secs_f64();
        let samples = secs * self.descriptor.sample_rate.get() as f64;
        self.seek_sample(samples as u64)
    }
}

impl super::NodeHandleSealed for SampleSourcePlayerNode {
    fn get_id(&self) -> UniqueId {
        self.internal_handle.object_id
    }
}

impl super::NodeHandle for SampleSourcePlayerNode {}

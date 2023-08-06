use std::borrow::Cow;

use ahash::{HashMap, HashMapExt};
use arrayvec::ArrayVec;

use crate::background_drop::BackgroundDrop;
use crate::channel_format::ChannelFormat;
use crate::command::*;
use crate::config::*;
use crate::data_structures::Graph;
use crate::data_structures::*;
use crate::nodes::*;
use crate::unique_id::UniqueId;

/// Services a server may offer to a consumer on the audio thread.
pub(crate) struct AudioThreadServerServices {
    pub(crate) block_allocator: BlockAllocator,

    /// A place where nodes store their output data.
    pub(crate) input_data: HashMap<UniqueId, NodeInputsData>,
}

pub(crate) struct RuntimeServerConfig {
    /// The output format of the audio device.
    output_format: ChannelFormat,
}

/// Options used when building a server.
///
/// `Default::default()` should be sufficient for all but the most demanding applications.
pub struct ServerOptions {
    /// Estimate of the expected number of nodes on the server's root graph.
    ///
    /// This should be large enough that your application never goes over it.  If it does, your application will potentially glitch briefly while Synthizer reallocates, especially on mobile platforms.
    pub expected_nodes: usize,

    /// Number of connections which are expected on the server's root graph.
    ///
    /// Going over this may cause glitching while Synthizer reallocates, especially on mobile platforms.
    pub expected_connections: usize,

    /// Internal knob to tune how many blocks the server-level block allocator makes available.  Unfortunately, this
    /// must be exposed for the moment.
    pub preallocated_blocks: usize,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            expected_nodes: 2048,
            expected_connections: 2048 * 16,
            // With current settings, about 33MB.  We will lower it and probably make it a const once we have buffer
            // reuse working.  In the common case this shouldn't need to be more than say 100, 1000 at most, but that
            // requires knowing how to do reference counting on blocks.
            preallocated_blocks: 35536,
        }
    }
}

/// Holds a node, as well as information needed to execute it.
struct NodeContainer {
    node: NodeHandle,

    /// Set to a unique value on every tick to serve as an inline marker as to whether or not this node has yet been
    /// run.
    ///
    /// This replaces external sets, which require allocation.
    executed_marker: UniqueId,
}

pub(crate) enum ServerCommand {
    RegisterNode {
        id: UniqueId,
        handle: NodeHandle,
        descriptor: Cow<'static, NodeDescriptor>,
    },
    DeregisterNode {
        id: UniqueId,
    },

    // Todo: actually we want a graph node that owns and handles graphs, but for now this is fine.
    UpdateGraph {
        new_graph: Graph,
    },
}

/// The implementation of a server, which is either executed inline or on an audio thread depending on user preference.
pub(crate) struct ServerImpl {
    nodes: HashMap<UniqueId, NodeContainer>,
    root_graph: BackgroundDrop<Graph>,

    services: AudioThreadServerServices,

    runtime_config: RuntimeServerConfig,

    /// The output buffers, which this server will temporarily write data to.
    output_blocks: ArrayVec<AllocatedBlock, MAX_CHANNELS>,

    /// How many samples are in the output blocks right now?
    output_frames_available: usize,

    /// Interleaved output data, which we need because the external world expects interleaved output data.
    interleaved_output_frames: Vec<f32>,

    /// Memoized node descriptors.
    pub(crate) memoized_descriptors: HashMap<UniqueId, Cow<'static, NodeDescriptor>>,
}

impl ServerImpl {
    pub fn new(output_format: ChannelFormat, opts: ServerOptions) -> Self {
        let mut ret = Self {
            nodes: HashMap::with_capacity(opts.expected_nodes),
            root_graph: BackgroundDrop::new(Graph::new()),
            output_blocks: ArrayVec::new(),
            output_frames_available: 0,
            interleaved_output_frames: Vec::with_capacity(MAX_CHANNELS * BLOCK_SIZE),
            services: AudioThreadServerServices {
                block_allocator: BlockAllocator::new(opts.preallocated_blocks),
                input_data: HashMap::with_capacity(opts.expected_nodes * MAX_INPUTS),
            },
            runtime_config: RuntimeServerConfig { output_format },
            memoized_descriptors: HashMap::with_capacity(opts.expected_nodes),
        };

        for _ in 0..ret.runtime_config.output_format.get_channel_count().get() {
            ret.output_blocks
                .push(ret.services.block_allocator.allocate_block());
        }

        // Prepare to have the space needed to interleave from our internal un-interleaved format to what miniaudio and
        // other audio libraries want.
        ret.interleaved_output_frames
            .resize(MAX_CHANNELS * BLOCK_SIZE, 0.0);
        ret
    }

    /// Fill the server's output blocks with one block of data.
    fn fill_output_blocks(&mut self) {
        for b in self.output_blocks.iter_mut() {
            b.fill(0.0);
        }

        let marker = UniqueId::new();

        self.root_graph.traverse_execution_order(|id| {
            let n = self
                .nodes
                .get_mut(id)
                .expect("Attempt to execute unregistered node");
            if n.executed_marker == marker {
                return;
            }

            n.executed_marker = marker;
            n.node.execute_erased(&mut ErasedExecutionContext {
                id: *id,
                services: &mut self.services,
                graph: &self.root_graph,
                speaker_format: &self.runtime_config.output_format,
                speaker_outputs: &mut self.output_blocks[..],
                descriptors: &self.memoized_descriptors,
            });
        });

        self.output_frames_available = BLOCK_SIZE;
    }

    /// Fill a slice with output data.
    ///
    /// The slice must be a multiple of the server's channel format in length.
    pub(crate) fn fill_slice(&mut self, slice: &mut [f32]) {
        let mut done_frames: usize = 0;
        let frame_chans = self.runtime_config.output_format.get_channel_count().get();

        assert!(!slice.is_empty());
        assert_eq!(slice.len() % frame_chans, 0);

        let needed_frames = slice.len() / frame_chans;

        while done_frames < needed_frames {
            if self.output_frames_available == 0 {
                self.fill_output_blocks();
                crate::channel_conversion::interleave_blocks(
                    &mut self.output_blocks[..],
                    &mut self.interleaved_output_frames[..BLOCK_SIZE * frame_chans],
                );
            }

            let still_doing = needed_frames - done_frames;
            let can_do = still_doing.min(self.output_frames_available);

            let slice_start = done_frames * frame_chans;
            let slice_end = slice_start + can_do * frame_chans;
            let local_output_start = (BLOCK_SIZE - self.output_frames_available) * frame_chans;
            let local_output_end = local_output_start + can_do * frame_chans;

            slice[slice_start..slice_end].copy_from_slice(
                &self.interleaved_output_frames[local_output_start..local_output_end],
            );
            done_frames += can_do;
            self.output_frames_available -= can_do;
        }
    }

    fn run_server_command(&mut self, cmd: ServerCommand) {
        match cmd {
            ServerCommand::RegisterNode {
                id,
                handle,
                descriptor,
            } => {
                let old = self.nodes.insert(
                    id,
                    NodeContainer {
                        node: handle,
                        executed_marker: UniqueId::new(),
                    },
                );
                assert!(
                    old.is_none(),
                    "Logic error: attempt to register a node with the same id twice"
                );
                self.memoized_descriptors.insert(id, descriptor);
            }
            ServerCommand::DeregisterNode { id } => {
                let old = self.nodes.remove(&id);
                assert!(
                    old.is_some(),
                    "Logic error: attempt to deregister node which was never registered"
                );
                self.memoized_descriptors.remove(&id);
            }
            ServerCommand::UpdateGraph { new_graph } => {
                // todo: this deallocates. We must defer graph freeing to a background thread.
                self.root_graph = BackgroundDrop::new(new_graph);
            }
        }
    }

    pub(crate) fn dispatch_command(&mut self, command: crate::command::Command) {
        // Right now all we have are server commands.
        self.run_server_command(
            command
                .extract_payload_as::<ServerCommand>()
                .unwrap_or_else(|_| panic!("only server commands are supported")),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fill a vec with a sine wave, then check that it's right.
    #[test]
    fn basic_fill_vec() {
        const SAMPLES: usize = 20 * BLOCK_SIZE;
        const FREQ: f64 = 300.0;

        let expected = (0..SAMPLES)
            .flat_map(|x| {
                let tmp = (x as f64 * 2.0 * std::f64::consts::PI * FREQ / crate::config::SR as f64)
                    .sin() as f32;
                [tmp, tmp]
            })
            .collect::<Vec<f32>>();

        let mut got = vec![0.0f32; SAMPLES * 2];

        let mut implementation = ServerImpl::new(ChannelFormat::Stereo, Default::default());
        let pool = crate::data_structures::ObjectPool::new();
        let node = pool.allocate(crate::nodes::trig::TrigWaveform::new_sin(FREQ));
        let output = pool.allocate(crate::nodes::audio_output::AudioOutputNode::new(
            ChannelFormat::Stereo,
        ));
        let sin_id = UniqueId::new();
        let output_id = UniqueId::new();
        implementation.run_server_command(ServerCommand::RegisterNode {
            id: sin_id,
            descriptor: node.describe(),
            handle: node.into(),
        });
        implementation.run_server_command(ServerCommand::RegisterNode {
            id: output_id,
            descriptor: output.describe(),
            handle: output.into(),
        });
        implementation.root_graph.connect(sin_id, 0, output_id, 0);

        for slice in got[..].chunks_mut(BLOCK_SIZE * 2 + 100) {
            implementation.fill_slice(slice);
        }

        for (i, (g, e)) in got.into_iter().zip(expected.into_iter()).enumerate() {
            assert!(
                (g - e).abs() < 0.01,
                "Index {i} is too different: got={g}, expected={e}",
            );
        }
    }
}

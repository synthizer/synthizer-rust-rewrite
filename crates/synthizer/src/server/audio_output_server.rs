use std::num::NonZeroU32;
use std::sync::Arc;

use audio_synchronization::concurrent_slab::ExclusiveSlabRef;

use crate::command::*;
use crate::data_structures::object_pool::ObjectPool;
use crate::error::*;
use crate::internal_object_handle::ServerChannel;

use super::implementation::*;

const MAX_PENDING_COMMANDS: usize = 100000;

/// The Option is always [Some], and exists so that we have something which we can clear.  Only currently unused slots
/// in the queue are [None].
type CommandQueue =
    thingbuf::ThingBuf<Option<crate::command::Command>, crate::option_recycler::OptionRecycler>;

/// The audio thread (at) part of an audio output server.
struct AudioOutputServerAT {
    implementation: ServerImpl,
    command_queue: Arc<CommandQueue>,
}

pub struct AudioOutputServerInner {
    device: synthizer_miniaudio::DeviceHandle,
    command_queue: Arc<CommandQueue>,
    pool: ObjectPool,
}

/// A server which outputs audio to the audio thread.
pub struct AudioOutputServer {
    inner: Arc<AudioOutputServerInner>,
}

impl AudioOutputServer {
    pub fn new_with_default_device() -> Result<Self> {
        let command_queue = Arc::new(CommandQueue::with_recycle(
            MAX_PENDING_COMMANDS,
            crate::option_recycler::OptionRecycler,
        ));

        let mut implementation = ServerImpl::new(
            crate::channel_format::ChannelFormat::Stereo,
            Default::default(),
        );

        let command_queue_cloned = command_queue.clone();

        let mut dev = synthizer_miniaudio::open_default_output_device(
            &synthizer_miniaudio::DeviceOptions {
                channel_format: Some(synthizer_miniaudio::DeviceChannelFormat::Stereo),
                sample_rate: Some(NonZeroU32::new(44100).unwrap()),
            },
            move |_, dest| {
                crate::background_drop::mark_audio_thread();
                while let Some(cmd) = command_queue_cloned.pop() {
                    implementation.dispatch_command(cmd.unwrap());
                }
                implementation.fill_slice(dest);
            },
        )?;

        dev.start()?;

        let inner = AudioOutputServerInner {
            device: dev,
            command_queue,
            pool: ObjectPool::new(),
        };

        Ok(AudioOutputServer {
            inner: Arc::new(inner),
        })
    }

    pub(crate) fn send_command(&self, mut command: Command) {
        while let Err(e) = self.inner.command_queue.push(Some(command)) {
            command = e.into_inner().unwrap();
        }
    }

    pub(crate) fn allocate<T: std::any::Any + Send + Sync>(
        &self,
        new_val: T,
    ) -> ExclusiveSlabRef<T> {
        self.inner.pool.allocate(new_val)
    }
}

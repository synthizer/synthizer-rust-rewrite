use std::num::NonZeroU32;
use std::sync::Arc;

use audio_synchronization::concurrent_slab::ExclusiveSlabRef;

use crate::command::*;
use crate::data_structures::object_pool::ObjectPool;
use crate::error::*;
use crate::internal_object_handle::ServerChannel;

use super::implementation::*;

/// Handle to an audio thread, which is an audio device plus whatever infrastructure needed to drive it.
pub(crate) struct AudioThread {
    inner: Arc<AudioThreadInner>,
}

struct AudioThreadInner {
    device: synthizer_miniaudio::DeviceHandle,
}

impl AudioThread {
    pub(crate) fn new_with_default_device(mut callback: ServerExecutionCallback) -> Result<Self> {
        let mut dev = synthizer_miniaudio::open_default_output_device(
            &synthizer_miniaudio::DeviceOptions {
                channel_format: Some(synthizer_miniaudio::DeviceChannelFormat::Stereo),
                sample_rate: Some(NonZeroU32::new(44100).unwrap()),
            },
            move |_, dest| {
                callback(dest);
            },
        )?;

        dev.start()?;

        let inner = AudioThreadInner { device: dev };

        Ok(AudioThread {
            inner: Arc::new(inner),
        })
    }
}

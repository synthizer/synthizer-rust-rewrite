use std::sync::Arc;

use atomic_refcell::AtomicRefCell;

use crate::data_structures::{
    ArcStash, ChangeTracker, ChangeTrackerToken, DeferredArcSwap, GetArcStash,
};
use crate::loop_spec::LoopSpec;
use crate::sample_sources::{Descriptor, SampleSource, SampleSourceError};
use crate::worker_pool as wp;

use super::{
    cross_thread::{BackgroundSourceController, BackgroundSourceSampleReader},
    driver::Driver,
};

#[derive(Clone, Default)]
pub(crate) struct ExecutionConfig {
    stash: ArcStash<Self>,
    pub(crate) loop_spec: ChangeTracker<LoopSpec>,
    pub(crate) seek_to: ChangeTracker<Option<u64>>,
}

impl GetArcStash for ExecutionConfig {
    fn get_stash(&self) -> &ArcStash<Self> {
        &self.stash
    }
}

/// If on the audio thread we:
///
/// - Update config through DeferredArcSwap
/// - Read samples with interior mutability.
struct InlineExecutionLocation {
    audio_thred: AtomicRefCell<InlineExecutionAudioFields>,
    config: DeferredArcSwap<ExecutionConfig>,
}

struct InlineExecutionAudioFields {
    driver: Driver,
    seek_token: ChangeTrackerToken<Option<u64>>,
    loop_token: ChangeTrackerToken<LoopSpec>,
}

#[allow(clippy::large_enum_variant)]
enum ExecutionLocation {
    Inline(InlineExecutionLocation),
    CrossThread {
        handle: AtomicRefCell<BackgroundSourceSampleReader>,
        controller: BackgroundSourceController,
    },
}

/// Responsible for running sources in a way which is safe for an audio thread.
///
/// When possible, this will run the source inline.  Otherwise, the source gets run on a background thread pool, and the
/// latency of decoding is increased.
///
/// See the comments on the public API [SampleSource] and the other pieces for when sources run in the background.
///
/// This uses interior mutability.  It is safe to call configuration methods from any thread but the audio thread.  It
/// is not safe to call configuration methods from the audio thread.  All reading must happen on the audio thread.
pub(crate) struct Executor {
    location: ExecutionLocation,
    descriptor: Descriptor,
}

impl Executor {
    pub(crate) fn new<S: SampleSource>(
        worker_pool: &wp::WorkerPoolHandle,
        source: S,
    ) -> crate::error::Result<Self> {
        let driver = Driver::new(source)?;
        let descriptor = driver.descriptor().clone().clone();

        let location = if driver.descriptor().latency.is_audio_thread_safe() {
            ExecutionLocation::Inline(InlineExecutionLocation {
                audio_thred: AtomicRefCell::new(InlineExecutionAudioFields {
                    driver,
                    seek_token: ChangeTrackerToken::new(),
                    loop_token: ChangeTrackerToken::new(),
                }),
                config: Default::default(),
            })
        } else {
            let (handle, controller) = super::cross_thread::new_in_pool(worker_pool, driver)?;
            ExecutionLocation::CrossThread {
                handle: AtomicRefCell::new(handle),
                controller,
            }
        };

        Ok(Self {
            location,
            descriptor,
        })
    }

    /// Read
    pub(crate) fn read_block(&self, destination: &mut [f32]) -> Result<u64, SampleSourceError> {
        match &self.location {
            ExecutionLocation::Inline(d) => {
                let cfg = d.config.load_full();
                let mut at = d.audio_thred.borrow_mut();
                if let Some((val, t)) = cfg.loop_spec.get_if_changed(&at.loop_token) {
                    at.driver.config_looping(*val);
                    at.loop_token = t;
                }
                at.driver.read_samples(destination)
            }
            ExecutionLocation::CrossThread { handle, .. } => {
                handle.borrow_mut().read_block(destination)
            }
        }
    }

    pub(crate) fn config_looping(&self, spec: LoopSpec) {
        match &self.location {
            ExecutionLocation::Inline(d) => {
                let mut new_cfg = d.config.load_full();
                Arc::make_mut(&mut new_cfg).loop_spec.replace(spec);
                d.config.publish(new_cfg);
            }
            ExecutionLocation::CrossThread { controller, .. } => controller.config_looping(spec),
        }
    }

    pub(crate) fn seek(&self, new_pos: u64) {
        match &self.location {
            ExecutionLocation::Inline(loc) => {
                let mut cfg = loc.config.load_full();
                Arc::make_mut(&mut cfg).seek_to.replace(Some(new_pos));
                loc.config.publish(cfg);
            }
            ExecutionLocation::CrossThread { controller, .. } => controller.seek(new_pos),
        }
    }

    pub(crate) fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
}

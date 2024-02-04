use crate::loop_spec::LoopSpec;
use crate::sample_sources::{Descriptor, SampleSource, SampleSourceError};
use crate::worker_pool as wp;

use super::{cross_thread::BackgroundSourceHandle, driver::Driver};

#[allow(clippy::large_enum_variant)]
enum ExecutionLocation {
    Inline(Driver),
    CrossThread(BackgroundSourceHandle),
}

/// Responsible for running sources in a way which is safe for an audio thread.
///
/// When possible, this will run the source inline.  Otherwise, the source gets run on a background thread pool, and the
/// latency of decoding is increased.
///
/// See the comments on the public API [SampleSource] and the other pieces for when sources run in the background.
pub(crate) struct Executor {
    location: ExecutionLocation,
}

impl Executor {
    pub(crate) fn new<S: SampleSource>(
        worker_pool: &wp::WorkerPoolHandle,
        source: S,
    ) -> Result<Self, SampleSourceError> {
        let driver = Driver::new(source)?;
        let location = if driver.descriptor().latency.is_audio_thread_safe() {
            ExecutionLocation::Inline(driver)
        } else {
            let handle = BackgroundSourceHandle::new_in_pool(worker_pool, driver);
            ExecutionLocation::CrossThread(handle)
        };

        Ok(Self { location })
    }

    /// Read
    pub(crate) fn read_block(&mut self, destination: &mut [f32]) -> Result<u64, SampleSourceError> {
        match &mut self.location {
            ExecutionLocation::Inline(d) => d.read_samples(destination),
            ExecutionLocation::CrossThread(c) => c.read_block(destination),
        }
    }

    pub(crate) fn config_looping(&mut self, spec: LoopSpec) {
        match &mut self.location {
            ExecutionLocation::Inline(d) => d.config_looping(spec),
            ExecutionLocation::CrossThread(c) => c.config_looping(spec),
        }
    }

    pub(crate) fn descriptor(&self) -> &Descriptor {
        match self.location {
            ExecutionLocation::Inline(ref d) => d.descriptor(),
            ExecutionLocation::CrossThread(ref c) => c.descriptor(),
        }
    }
}

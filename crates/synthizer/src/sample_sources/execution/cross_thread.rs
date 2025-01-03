//! Implements support for running a source on a background thread.
use std::sync::Arc;

use audio_synchronization::spsc_ring as sring;

use crate::config::BLOCK_SIZE;
use crate::data_structures::ChangeTrackerToken;
use crate::error::Result;
use crate::loop_spec::LoopSpec;
use crate::sample_sources::{Descriptor, SampleSourceError};
use crate::worker_pool as wp;

use super::driver::Driver;
use super::executor::ExecutionConfig;

/// Number of samples to be latent by.
const LATENCY: usize = 4410;

type ConfigHolder = Arc<arc_swap::ArcSwap<ExecutionConfig>>;

/// The task which is run in the background thread.
///
/// This has three cases:
///
/// - If the source is progressing, it's writing data.
/// - If the source is at the end, it's writing zeros (for now; if that becomes too slow that's something we can change,
///   but that's the bug-free simple option).
/// - If the source dies because of an error, it's doing nothing and closes the ring.
struct Task {
    /// Handles are closed by dropping them, so this becomes `None` if the source dies.
    ring_handle: Option<sring::RingWriter<f32>>,

    config: ConfigHolder,
    loop_config_token: ChangeTrackerToken<LoopSpec>,
    seek_token: ChangeTrackerToken<Option<u64>>,

    driver: Driver,
}

/// Handle through which one may read samples.
pub(super) struct BackgroundSourceSampleReader {
    /// When we see that the writer is closed, goes to `None` so that the underlying ring can free the memory.
    reader: Option<sring::RingReader<f32>>,

    config: ConfigHolder,

    /// Copy of the descriptor, which would otherwise only be on the background thread.
    descriptor: Descriptor,

    /// Holds the task alive to prevent cancellation.
    worker_task_handle: wp::TaskHandle,
}

/// Allows any thread(s) to configure the background source.
///
/// Does not keep the background source alive.
pub(super) struct BackgroundSourceController {
    config: ConfigHolder,
}

impl Task {
    /// Pick up config changes.
    fn update_config(&mut self) -> Result<(), SampleSourceError> {
        let cfg = self.config.load();

        if let Some((val, t)) = cfg.loop_spec.get_if_changed(&self.loop_config_token) {
            self.driver.config_looping(*val);
            self.loop_config_token = t;
        }

        if let Some((seek_to, t)) = cfg.seek_to.get_if_changed(&self.seek_token) {
            if let Some(pos) = seek_to {
                self.driver.seek(*pos)?;
            }
            self.seek_token = t;
        }

        Ok(())
    }

    /// Move this task forward by one block of data, returning any errors which may result.
    fn tick_fallible(&mut self) -> Result<(), SampleSourceError> {
        let chans = self.driver.descriptor().get_channel_count();
        let needed = BLOCK_SIZE * chans;

        self.update_config()?;

        let ring = self.ring_handle.as_mut().expect("Should be set unless the ring is closed, in which case tick_fallible shouldn't get called");

        // We have to work around the design of the ring writer so that we can funnel a result out of the callback on
        // error.
        let mut ret_result: Result<(), SampleSourceError> = Ok(());

        ring.write_slices(|slices| {
            let Some((first, mut second)) = slices else {
                return 0;
            };

            // This is the first place we decided to use the spsc ring from audio_synchronization heavily, so let's be
            // paranoid and assert.
            assert_eq!(first.len() % needed, 0);
            // If the ring gave us a slice at all, it should be at least one block.
            assert!(first.len() >= needed);

            // The driver writes blocks. We get a multiple of blocks from the ring, so windows() etc. work.
            //
            // We zero blocks so that partial blocks won't just leave data around.  This is already a cross-thread ring
            // doing disk I/O; that's not expensive by comparison.
            for block in first
                .chunks_mut(needed)
                .chain(second.iter_mut().flat_map(|x| x.chunks_mut(needed)))
            {
                block.fill(0.0);

                // We'll just always do zeros, so if we errored out take the hit this time and the logic one level up will never call this again anyway.
                if ret_result.is_err() {
                    continue;
                }

                ret_result = self.driver.read_samples(block).map(|_| ());
            }

            first.len() + second.map(|x| x.len()).unwrap_or(0)
        });

        ret_result
    }

    /// Tick this source forward if needed.
    ///
    /// If an error occurs, return false.
    fn tick(&mut self) -> bool {
        if self.ring_handle.is_none() {
            // A previous tick failed, so we must abort.
            return false;
        }

        if let Err(e) = self.tick_fallible() {
            log::error!("Closing background thread task for streaming SampleSource because the source errored with {}", e);
            self.ring_handle = None;
            return false;
        }

        true
    }
}

impl wp::Task for Task {
    fn execute(&mut self) -> bool {
        self.tick()
    }

    fn priority(&self) -> crate::worker_pool::TaskPriority {
        crate::worker_pool::TaskPriority::Decoding(0)
    }
}

impl BackgroundSourceSampleReader {
    /// Read exactly one block of audio data.
    pub(super) fn read_block(&mut self, destination: &mut [f32]) -> Result<u64, SampleSourceError> {
        // Actually this is infallible, but errorability is good practice.
        let chans = self.descriptor.get_channel_count();
        let needed = BLOCK_SIZE * chans;

        let mut did = 0;

        self.reader.as_mut().unwrap().read_slices(|slices| {
            let Some((first, _)) = slices else {
                did = 0;
                return 0;
            };

            assert!(first.len() >= needed);
            assert_eq!(first.len() % needed, 0);

            destination.copy_from_slice(&first[..needed]);
            did = needed;
            needed
        });

        destination[did..].fill(0.0);

        assert_eq!(did % chans, 0);
        Ok((did / chans) as u64)
    }
}

impl BackgroundSourceController {
    pub(super) fn config_looping(&self, loop_spec: LoopSpec) {
        self.config.rcu(|val| {
            let mut val = val.clone();
            let nv = Arc::make_mut(&mut val);
            nv.loop_spec.replace(loop_spec);
            val
        });
    }

    pub(super) fn seek(&self, new_pos: u64) {
        self.config.rcu(|val| {
            let mut val = val.clone();
            let nv = Arc::make_mut(&mut val);
            nv.seek_to.replace(Some(new_pos));
            val
        });
    }
}

pub(super) fn new_in_pool(
    pool: &wp::WorkerPoolHandle,
    driver: Driver,
) -> Result<(BackgroundSourceSampleReader, BackgroundSourceController)> {
    let descriptor = driver.descriptor().clone();
    let cfg = ConfigHolder::default();

    let latency_rounded_up = LATENCY.next_multiple_of(BLOCK_SIZE);
    let (sring_reader, sring_writer) =
        sring::create_ring(latency_rounded_up * descriptor.get_channel_count());

    let task = Task {
        driver,
        ring_handle: Some(sring_writer),
        config: cfg.clone(),
        loop_config_token: Default::default(),
        seek_token: Default::default(),
    };

    let worker_task_handle = pool.register_task(task);

    Ok((
        BackgroundSourceSampleReader {
            config: cfg.clone(),
            reader: Some(sring_reader),
            descriptor,
            worker_task_handle,
        },
        BackgroundSourceController { config: cfg },
    ))
}

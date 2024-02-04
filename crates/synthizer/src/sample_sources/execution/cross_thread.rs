//! Implements support for running a source on a background thread.
use std::sync::Arc;

use audio_synchronization::spsc_ring as sring;

use crate::config::BLOCK_SIZE;
use crate::loop_spec::LoopSpec;
use crate::option_recycler::OptionRecycler;
use crate::sample_sources::{Descriptor, SampleSourceError};
use crate::worker_pool as wp;

use super::driver::Driver;

// Design notes:
//
// First, samples go back over an uninterleaved ring, which always contains single blocks of data, even if zeroing is
// required.  For streaming sources, we can push pitch bending to the background thread if we ever decide to go there,
// then tell users if they want no latency they need to use a buffer.
//
// This leaves the question of whatb to do in two cases: what happens when the source dies?  How do we get user commands
// to the background thread?
//
// Source death is easy.  In that case, we simply close the spsc ring (which is why audio_synchronization supports
// closing).
//
// Getting commands across is a  bit more complex.  Observe that we have the operations of seeking and configuring
// loops, and suppose that the user does both in one audio tick.  It doesn't matter which order they happen in.  Indeed,
// it never matters even across multiple blocks.  In this way, the operations are orthogonal.  It also doesn't matter
// whether or not multiple operations exist if they arrive at the background task at the same time, because while we
// might run them all, only the final ones apply.  This means that we can coalesce commands into a struct of orthogonal
// operations, rather than dealing with a list of commands.  We can merge those structs together as we go, and then send
// exactly one struct across per audio tick, then merge it in the background task if the background task gets more than
// one because it fell behind.  This lets us use a very small command ring.
//
// Unfortunately we must use thingbuf for the command queue. This is because our types are too complex to be pod, and so
// we cannot use the audio_synchronization ring which is designed primarily for sample data and so assumes zeroing
// memory is valid initialization.
//
// The interleaving versus uninterleaving question here chooses uninterleaving because the common case requires
// resampling, currently via Rubato, and that forces uninterleaved data.  There's no point interleaving that, then
// uninterleaving it one more time on the audio thread.
//
// Recall that the ring works such that one can always work in a multiple on both sides, and if doing so always get
// single slices which are one block in length.  We use that here.

const COMMAND_QUEUE_LEN: usize = 16;

/// Number of samples to be latent by.
const LATENCY: usize = 4410;

type CommandQueue = Arc<thingbuf::ThingBuf<Option<ConfigPatch>, OptionRecycler>>;

/// Given two fields, return the first which is not `None`.
fn merge_opts<T>(first: Option<T>, second: Option<T>) -> Option<T> {
    match (first, second) {
        (Some(x), None) => Some(x),
        (None, Some(x)) => Some(x),
        (Some(_), Some(x)) => Some(x),
        (None, None) => None,
    }
}

/// A patch to apply to the background thread.
///
/// TODO: seeking is next, we're handling loops first.
#[derive(Clone, Debug, Default)]
struct ConfigPatch {
    loop_spec: Option<LoopSpec>,
}

impl ConfigPatch {
    /// Merge this patch with `newer`, so that the output patch has the fields from `newer` if those fields are set,
    /// otherwise the fields from ourself.
    fn merge_newer(self, newer: ConfigPatch) -> Self {
        Self {
            loop_spec: merge_opts(self.loop_spec, newer.loop_spec),
        }
    }
}

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

    command_queue: CommandQueue,

    driver: Driver,
}

/// Note: pending patches only move to the background thread when `read` is called.  Otherwise, it's incrementally
/// building them locally.
pub(super) struct BackgroundSourceHandle {
    /// When we see that the writer is closed, goes to `None` so that the underlying ring can free the memory.
    reader: Option<sring::RingReader<f32>>,

    /// Copy of the descriptor, which would otherwise only be on the background thread.
    descriptor: Descriptor,

    /// Holds the task alive to prevent cancellation.
    worker_task_handle: wp::TaskHandle,

    command_queue: CommandQueue,

    next_patch: Option<ConfigPatch>,
}

impl Task {
    /// Get any config patches we may wish to apply this time.
    fn get_config_patch(&self) -> ConfigPatch {
        let mut ret = ConfigPatch::default();

        while let Some(p) = self.command_queue.pop() {
            let p  = p.expect("The command queue uses Option because thingbuf needs a recycler, but we only ever write Some to it");
            ret = ret.merge_newer(p);
        }

        ret
    }

    /// Apply the set fields in the config patch to the driver.
    fn apply_patch(&mut self, patch: ConfigPatch) -> Result<(), SampleSourceError> {
        if let Some(loop_spec) = patch.loop_spec {
            self.driver.config_looping(loop_spec);
        }
        Ok(())
    }

    /// Move this task forward by one block of data, returning any errors which may result.
    fn tick_fallible(&mut self) -> Result<(), SampleSourceError> {
        let chans = self.driver.descriptor().get_channel_count();
        let needed = BLOCK_SIZE * chans;

        let patch = self.get_config_patch();
        self.apply_patch(patch)?;

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

            // The driver writes blocks. We get a multiple of blocks from the ring, so windows() etc. work.  The driver
            // fills blocks with zeros if there's nothing to do.  Q.E.D. iterate over block-sized subslices all the way.
            for block in first
                .chunks_mut(needed)
                .chain(second.iter_mut().flat_map(|x| x.chunks_mut(needed)))
            {
                // We'll just always do zeros, so if we errored out take the hit this time and the logic one level up will never call this again anyway.
                if ret_result.is_err() {
                    block.fill(0.0);
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
    /// If the channel is closed or an error occurs, return false.
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

impl BackgroundSourceHandle {
    pub(super) fn new_in_pool(pool: &wp::WorkerPoolHandle, driver: Driver) -> Self {
        let descriptor = driver.descriptor().clone();
        let command_queue = Arc::new(thingbuf::ThingBuf::with_recycle(
            COMMAND_QUEUE_LEN,
            OptionRecycler,
        ));
        let latency_rounded_up = LATENCY.next_multiple_of(BLOCK_SIZE);
        let (sring_reader, sring_writer) =
            sring::create_ring(latency_rounded_up * descriptor.get_channel_count());

        let task = Task {
            command_queue: command_queue.clone(),
            driver,
            ring_handle: Some(sring_writer),
        };

        let worker_task_handle = pool.register_task(task);

        BackgroundSourceHandle {
            command_queue,
            reader: Some(sring_reader),
            descriptor,
            worker_task_handle,
            next_patch: None,
        }
    }

    /// Read exactly one block of audio data.
    pub(super) fn read_block(&mut self, destination: &mut [f32]) -> Result<u64, SampleSourceError> {
        // Actually this is infallible, but errorability is good practice.
        let chans = self.descriptor.get_channel_count();
        let needed = BLOCK_SIZE * chans;

        // Before doing anything else, push the pending patch if necessary.
        if let Some(p) = self.next_patch.as_ref() {
            if self.command_queue.push(Some((*p).clone())).is_ok() {
                // We sent, so we don't need to accumulate into this config anymore.
                self.next_patch = None;
            }

            // Else, we'll just accumulate more changes and get them next time.
        }

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

    fn get_next_patch_mut(&mut self) -> &mut ConfigPatch {
        self.next_patch.get_or_insert_with(Default::default)
    }

    pub(super) fn config_looping(&mut self, loop_spec: LoopSpec) {
        self.get_next_patch_mut().loop_spec = Some(loop_spec);
    }

    pub(crate) fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
}

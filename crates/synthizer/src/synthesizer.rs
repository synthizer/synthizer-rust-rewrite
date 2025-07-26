use std::any::Any;
use std::marker::PhantomData as PD;
use std::sync::Arc;
use std::time::Duration;

use atomic_refcell::AtomicRefCell;
use rpds::{HashTrieMapSync, VectorSync};

use crate::config;
use crate::core_traits::*;
use crate::cpal_device::{AudioDevice, DeviceOptions};
use crate::error::{Error, Result};
use crate::mount_point::ErasedMountPoint;
use crate::sample_sources::{execution::Executor as MediaExecutor, SampleSource};
use crate::signals as sigs;
use crate::signals::{
    MediaEntry, MediaExecutorMap, Slot, SlotMap, SlotUpdateContext, SlotValueContainer,
};
use crate::unique_id::UniqueId;

type SynthMap<K, V> = HashTrieMapSync<K, V>;
type SynthVec<T> = VectorSync<T>;

pub struct Synthesizer {
    state: Arc<crate::data_structures::deferred_arc_swap::DeferredArcSwap<SynthesizerState>>,

    device: Option<AudioDevice>,

    worker_pool: crate::worker_pool::WorkerPoolHandle,
}

#[derive(Clone)]
pub(crate) struct MountContainer {
    pub(crate) pending_drop: Arc<std::sync::atomic::AtomicBool>,

    /// Should only be accessed from the audio thread.  Cloning is fine.
    pub(crate) erased_mount: Arc<AtomicRefCell<Box<dyn ErasedMountPoint>>>,

    pub(crate) slots: SlotMap<UniqueId, Arc<dyn Any + Send + Sync + 'static>>,
    pub(crate) media: MediaExecutorMap,
}

/// This is the state published to the audio thread.
///
/// Here's how this works: the state is behind `Arc`.  To manipulate it, we `make_mut` that `Arc`, do whatever we're
/// doing, then publish via `ArcSwap`.  This is "cheap" to clone in the sense that it's using persistent data
/// structures, but the way things actually work is we have batches and each batch will modify only one copy until the
/// batch ends.  On the audio thread, interior mutability is then used to modify the states.
///
/// That's a lot of pointer chasing.  To deal with that, mounts materialize once per block and run the block.
///
/// To make sure deallocation never happens on the audio thread, we maintain a linked list of these.  When a new state
/// is seen by the audio thread, the old state will have been swapped into place.  Then, when a batch starts, we drop
/// that linked list if needed.  As a result, the final Arc never goes away on the audio thread.  There is a kind of
/// trick however: this can indeed be a cycle.  We rely on the next batch creation to clear that cycle.
#[derive(Clone)]
pub(crate) struct SynthesizerState {
    arc_freelist: crate::data_structures::deferred_arc_swap::ArcStash<Self>,

    pub(crate) mounts: SynthMap<UniqueId, MountContainer>,

    pub(crate) audio_thred_state: Arc<AtomicRefCell<AudioThreadState>>,
}

impl crate::data_structures::deferred_arc_swap::GetArcStash for SynthesizerState {
    fn get_stash(&self) -> &crate::data_structures::deferred_arc_swap::ArcStash<Self> {
        &self.arc_freelist
    }
}

/// Ephemeral state for the audio thread itself.  Owned by the audio thread but behind AtomicRefCell to avoid unsafe
/// code.
pub(crate) struct AudioThreadState {
    /// Intermediate stereo buffer before going to the audio device.
    ///
    /// This will be more complex a bit later on. At the moment, it's more "get us off the ground" stuff.
    pub(crate) buffer: [[f64; 2]; config::BLOCK_SIZE],

    pub(crate) buf_remaining: usize,

    /// time in blocks since this state was created.
    pub(crate) time_in_blocks: u64,
}

/// Internal helper type: flips a boolean to true when dropped.
///
/// This allows dropping outside the batch context. The object(s) are marked dropped and then, on the next batch, they
/// are actually removed.  Eventually they drop for real, as states rotate out of the audio thread.
struct MarkDropped(Arc<std::sync::atomic::AtomicBool>);

impl MarkDropped {
    pub(crate) fn new() -> Self {
        MarkDropped(Arc::new(std::sync::atomic::AtomicBool::new(false)))
    }
}

impl Drop for MarkDropped {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// A handle which may be used to manipulate some object.
///
/// Handles keep objects alive.  When the last handle drops, the object does as well.
pub struct Handle {
    object_id: UniqueId,
    mark_drop: Arc<MarkDropped>,
}

impl Clone for Handle {
    fn clone(&self) -> Self {
        Self {
            object_id: self.object_id,
            mark_drop: self.mark_drop.clone(),
        }
    }
}

/// A batch of changes for the audio thread.
///
/// You ask for a batch.  Then you manipulate things by asking the batch for their parameters.  The states all build up,
/// and are stored with the batch.  Then, when the batch drops, they are published to the audio thread
pub struct Batch<'a> {
    synthesizer: &'a mut Synthesizer,
    new_state: SynthesizerState,
}

impl Drop for Batch<'_> {
    fn drop(&mut self) {
        self.handle_pending_drops();
        self.synthesizer
            .state
            .publish(Arc::new(self.new_state.clone()));
    }
}

impl Synthesizer {
    pub fn new_default_output() -> Result<Self> {
        let opts = DeviceOptions {
            sample_rate: None, // Let the device choose its preferred sample rate
            channels: Some(2), // Stereo
        };

        let state = Arc::new(
            crate::data_structures::deferred_arc_swap::DeferredArcSwap::new(Arc::new(
                SynthesizerState::new(),
            )),
        );

        let worker_pool =
            crate::worker_pool::WorkerPoolHandle::new_threaded(config::WORKER_POOL_THREADS);
        let dev = {
            let published_state = state.clone();
            let wp_cloned = worker_pool.clone();
            
            // Buffer to handle mismatch between cpal's requested frame count and our BLOCK_SIZE
            let mut remainder_buffer = Vec::with_capacity(config::BLOCK_SIZE * 2);
            
            AudioDevice::open_default(opts, move |dest| {
                let mut dest_offset = 0;
                
                // First, drain any remainder from the previous callback
                let remainder_to_copy = remainder_buffer.len().min(dest.len());
                if remainder_to_copy > 0 {
                    dest[..remainder_to_copy].copy_from_slice(&remainder_buffer[..remainder_to_copy]);
                    remainder_buffer.drain(..remainder_to_copy);
                    dest_offset = remainder_to_copy;
                }
                
                // Process remaining samples
                let mut remaining_dest = &mut dest[dest_offset..];
                while !remaining_dest.is_empty() {
                    // We need to process in BLOCK_SIZE * 2 chunks (stereo)
                    let block_size_samples = config::BLOCK_SIZE * 2;
                    
                    if remaining_dest.len() >= block_size_samples {
                        // Process directly into the destination
                        let update = published_state.load_full();
                        at_iter(&update, &mut remaining_dest[..block_size_samples]);
                        published_state.defer_reclaim(update);
                        wp_cloned.signal_audio_tick_complete();
                        remaining_dest = &mut remaining_dest[block_size_samples..];
                    } else {
                        // Not enough space for a full block, generate into our buffer
                        let mut temp_buffer = vec![0.0f32; block_size_samples];
                        let update = published_state.load_full();
                        at_iter(&update, &mut temp_buffer);
                        published_state.defer_reclaim(update);
                        wp_cloned.signal_audio_tick_complete();
                        
                        // Copy what we can to the destination
                        let to_copy = remaining_dest.len();
                        remaining_dest.copy_from_slice(&temp_buffer[..to_copy]);
                        
                        // Save the rest for next callback
                        remainder_buffer.extend_from_slice(&temp_buffer[to_copy..]);
                        remaining_dest = &mut [];
                    }
                }
            })?
        };

        dev.start()?;

        Ok(Self {
            state,
            device: Some(dev),
            worker_pool,
        })
    }

    pub fn batch(&mut self) -> Batch<'_> {
        let new_state = Arc::unwrap_or_clone(self.state.load_full());

        let mut ret = Batch {
            synthesizer: self,
            new_state,
        };

        ret.handle_pending_drops();
        ret
    }

    /// Convert a duration to time in samples, rounding up.
    pub fn duration_to_samples(&self, dur: Duration) -> usize {
        let s = dur.as_secs_f64();
        let ret = (s * config::SR as f64).ceil() as usize;
        debug_assert!(Duration::from_secs_f64(ret as f64 / config::SR as f64) >= dur);
        ret
    }
}

impl SynthesizerState {
    fn new() -> Self {
        Self {
            arc_freelist: Default::default(),
            audio_thred_state: Arc::new(AtomicRefCell::new(AudioThreadState {
                buf_remaining: 0,
                buffer: [[0.0f64; 2]; config::BLOCK_SIZE],
                time_in_blocks: 0,
            })),
            mounts: SynthMap::new_sync(),
        }
    }
}

impl Batch<'_> {
    /// Called on batch creation to catch pending drops from last time, then again on batch publish to catch pending
    /// drops the user might have made during the batch.
    fn handle_pending_drops(&mut self) {
        // no retain in rpds.
        let mut pending_dropkeys = smallvec::SmallVec::<[UniqueId; 16]>::new();

        for (id, m) in self.new_state.mounts.iter() {
            if m.pending_drop.load(std::sync::atomic::Ordering::Relaxed) {
                pending_dropkeys.push(*id);
            }
        }

        for id in pending_dropkeys {
            self.new_state.mounts.remove_mut(&id);
        }
    }

    pub fn mount<M: crate::mount_point::Mountable>(&mut self, mut new_mount: M) -> Result<Handle> {
        let mut slots = SlotMap::new_sync();
        let mut media = MediaExecutorMap::new_sync();

        new_mount.trace(&mut |id, val| match val {
            TracedResource::Slot(s) => slots.insert_mut(id, s),
            TracedResource::Media(m) => media.insert_mut(id, MediaEntry::new(m)),
        })?;

        let object_id = UniqueId::new();
        let mount = new_mount.into_mount(self)?;

        let pending_drop = MarkDropped::new();

        let inserting = MountContainer {
            erased_mount: Arc::new(AtomicRefCell::new(mount)),
            pending_drop: pending_drop.0.clone(),
            slots,
            media,
        };
        self.new_state.mounts.insert_mut(object_id, inserting);

        Ok(Handle {
            object_id,
            mark_drop: Arc::new(pending_drop),
        })
    }

    /// Allocate a slot.
    ///
    /// This slot cannot be used until a chain which uses it is mounted.  To use a slot, call [Slot::signal()] or
    /// variations, specifying an initial value.  Mounting activates the slot by allocating the necessary internal data
    /// structures.
    pub fn allocate_slot<T>(&mut self) -> Slot<T> {
        Slot {
            slot_id: UniqueId::new(),
            _phantom: PD,
        }
    }

    pub fn replace_slot_value<T>(
        &mut self,
        handle: &Handle,
        slot: &Slot<T>,
        new_val: T,
    ) -> Result<()>
    where
        T: Send + Sync + Clone + 'static,
    {
        let slot  = self.new_state
        .mounts.get_mut(&handle.object_id)
        .expect("We give out handles, so the user shouldn't be able to get one to objects that don't exist")
        .slots
        .get_mut(&slot.slot_id)
        .ok_or_else(||Error::new_validation_cow("Slot does not match this mount"))?;
        let newslot = slot
            .downcast_ref::<SlotValueContainer<T>>()
            .unwrap()
            .replace(new_val);
        *slot = Arc::new(newslot);

        Ok(())
    }

    /// Convert a [SampleSource] into media in this synthesizer, and return a reference to it.
    ///
    /// The reference is not valid for media operations until a signal referencing it is mounted.
    pub fn make_media<S>(&mut self, source: S) -> Result<sigs::Media>
    where
        S: SampleSource,
    {
        let executor = MediaExecutor::new(&self.synthesizer.worker_pool, source)?;

        Ok(sigs::Media {
            media_id: UniqueId::new(),
            descriptor: executor.descriptor().clone(),
            executor: Some(Arc::new(executor)),
        })
    }

    /// Seek some media on this handle.
    ///
    /// In some cases, this happens on an asynchronous background thread.  In all cases, the change will take place
    /// immediately, not when the batch is dropped.  This is one of the two operations discussed in [Media]'s docs as
    /// requiring an exception to our rule of applying all at once atomicly at batch drop.    ///
    /// Seeks past the end are converted to seeks to the end.
    ///
    /// If the underlying media is not seekable or some other I/O error results then an error is logged later from a
    /// background thread.
    pub fn media_seek(&mut self, handle: &Handle, media: &sigs::Media, new_pos: u64) -> Result<()> {
        let mount = self.new_state
        .mounts.get_mut(&handle.object_id)
        .expect("We give out handles, so the user shouldn't be able to get one to objects that don't exist");
        let media = mount
            .media
            .get(&media.media_id)
            .ok_or_else(|| Error::new_validation_cow("Media does not match this mount"))?;
        media.executor.seek(new_pos);
        Ok(())
    }

    /// Configure how this media loops.
    ///
    /// In some cases, this happens on an asynchronous background thread.  In all cases, the change will take place
    /// immediately, not when the batch is dropped.  This is one of the two operations discussed in [sigs::Media]'s docs
    /// as requiring an exception to our rule of applying all at once atomicly at batch drop.
    ///
    /// Disabling looping is done with [crate::LoopSpec::no_looping], not an alternative method on the batch.
    pub fn media_config_looping(
        &mut self,
        handle: &Handle,
        media: &sigs::Media,
        spec: crate::LoopSpec,
    ) -> Result<()> {
        let mount = self.new_state
        .mounts.get_mut(&handle.object_id)
        .expect("We give out handles, so the user shouldn't be able to get one to objects that don't exist");
        let media = mount
            .media
            .get(&media.media_id)
            .ok_or_else(|| Error::new_validation_cow("Media does not match this mount"))?;
        media.executor.config_looping(spec);
        Ok(())
    }

    /// Pause this media.
    ///
    /// If you don't have a mechanism for fadeout, this can click.
    pub fn media_pause(&mut self, handle: &Handle, media: &sigs::Media) -> Result<()> {
        let mount = self.new_state
        .mounts.get_mut(&handle.object_id)
        .expect("We give out handles, so the user shouldn't be able to get one to objects that don't exist");
        let media = mount
            .media
            .get_mut(&media.media_id)
            .ok_or_else(|| Error::new_validation_cow("Media does not match this mount"))?;
        media.playing = false;
        Ok(())
    }

    /// Start this media playing again.
    ///
    /// Media starts in the playing state, so this is only needed if pausing it.
    ///
    /// If you don't also put a mechanism in place for fade-in, this can click.
    pub fn media_play(&mut self, handle: &Handle, media: &sigs::Media) -> Result<()> {
        let mount = self.new_state
        .mounts.get_mut(&handle.object_id)
        .expect("We give out handles, so the user shouldn't be able to get one to objects that don't exist");
        let media = mount
            .media
            .get_mut(&media.media_id)
            .ok_or_else(|| Error::new_validation_cow("Media does not match this mount"))?;
        media.playing = true;
        Ok(())
    }

    pub fn duration_to_samples(&self, dur: Duration) -> usize {
        self.synthesizer.duration_to_samples(dur)
    }
}

/// Run one iteration of the audio thread.
fn at_iter(state: &Arc<SynthesizerState>, mut dest: &mut [f32]) {
    while !dest.is_empty() {
        // Grab the audio thread state and copy out whatever data we can.
        {
            let mut state = state.audio_thred_state.borrow_mut();
            if state.buf_remaining > 0 {
                // Hardcoded stereo, at the moment.
                let remaining = dest.len() / 2;
                let will_do = state.buf_remaining.min(remaining);
                assert!(will_do > 0);

                let start_ind = state.buffer.len() - state.buf_remaining;
                let grabbing = &mut state.buffer[start_ind..(start_ind + will_do)];
                grabbing.iter().enumerate().for_each(|(i, x)| {
                    dest[i * 2] = x[0] as f32;
                    dest[i * 2 + 1] = x[1] as f32;
                });
                // Careful: advance by stereo, not mono.
                dest = &mut dest[will_do * 2..];
                state.buf_remaining -= will_do;

                if dest.is_empty() {
                    // This was enough, and we do not need to refill the buffer.
                    return;
                }
            }
        }

        // Prepare the audio thread state, then release the borrow, allowing mount points to grab it.
        {
            let mut at_state = state.audio_thred_state.borrow_mut();

            at_state.buf_remaining = config::BLOCK_SIZE;
            // Zero it out for this iteration.
            at_state.buffer.fill([0.0f64; 2]);
        }

        let mut as_mut = state.audio_thred_state.borrow_mut();

        // Mounts may fill the audio buffer.
        for (id, m) in state.mounts.iter() {
            if m.pending_drop.load(std::sync::atomic::Ordering::Relaxed) {
                continue;
            }

            m.erased_mount.borrow_mut().run(
                state,
                id,
                &crate::context::FixedSignalExecutionContext {
                    time_in_blocks: as_mut.time_in_blocks,
                    audio_destinationh: atomic_refcell::AtomicRefCell::new(&mut as_mut.buffer),
                    audio_destination_format: &crate::channel_format::ChannelFormat::Stereo,
                    slots: &SlotUpdateContext {
                        mount_slots: &m.slots,
                    },
                    media: &m.media,
                },
            );
        }

        as_mut.time_in_blocks += 1;
    }
}

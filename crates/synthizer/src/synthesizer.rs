use std::any::Any;
use std::collections::VecDeque;
use std::marker::PhantomData as PD;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atomic_refcell::AtomicRefCell;

use crate::config;
use crate::core_traits::*;
use crate::cpal_device::{AudioDevice, DeviceOptions};
use crate::error::Result;
use crate::mount_point::ErasedMountPoint;
use crate::sample_sources::UnifiedMediaSource;
use crate::signals as sigs;
use crate::signals::{Slot, SlotUpdateContext, SlotValueContainer};
use crate::unique_id::UniqueId;

// Temporary type for compatibility with mount system
// Will be removed when mounts are updated to work with commands
pub(crate) struct SynthesizerState;

pub struct Synthesizer {
    command_queue: Arc<Mutex<VecDeque<Box<dyn crate::core_traits::Command>>>>,

    device: Option<AudioDevice>,

    worker_pool: crate::worker_pool::WorkerPoolHandle,
}

#[derive(Clone)]
pub(crate) struct MountContainer {
    pub(crate) pending_drop: Arc<std::sync::atomic::AtomicBool>,

    /// Should only be accessed from the audio thread.  Cloning is fine.
    pub(crate) erased_mount: Arc<AtomicRefCell<Box<dyn ErasedMountPoint>>>,
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

    /// Standard library version of mounts for audio thread processing
    pub(crate) mounts: std::collections::HashMap<UniqueId, MountContainer>,

    /// Global slots map for audio thread processing
    pub(crate) slots: std::collections::HashMap<UniqueId, Arc<dyn Any + Send + Sync + 'static>>,
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
/// You ask for a batch.  Then you manipulate things by asking the batch for their parameters.  The commands all build up,
/// and are stored with the batch.  Then, when the batch drops, they are sent to the audio thread
pub struct Batch<'a> {
    synthesizer: &'a mut Synthesizer,
    commands: Vec<Box<dyn crate::core_traits::Command>>,
}

impl Drop for Batch<'_> {
    fn drop(&mut self) {
        // Move all commands to the queue
        let mut queue = self.synthesizer.command_queue.lock().unwrap();
        queue.extend(self.commands.drain(..));
    }
}

impl Synthesizer {
    pub fn new_default_output() -> Result<Self> {
        let opts = DeviceOptions {
            sample_rate: None, // Let the device choose its preferred sample rate
            channels: Some(2), // Stereo
        };

        let command_queue = Arc::new(Mutex::new(VecDeque::new()));
        let worker_pool =
            crate::worker_pool::WorkerPoolHandle::new_threaded(config::WORKER_POOL_THREADS);
        
        let dev = {
            let queue_clone = command_queue.clone();
            let wp_cloned = worker_pool.clone();

            // Create persistent audio thread state
            let mut audio_state = AudioThreadState {
                buf_remaining: 0,
                buffer: [[0.0f64; 2]; config::BLOCK_SIZE],
                time_in_blocks: 0,
                mounts: std::collections::HashMap::new(),
                slots: std::collections::HashMap::new(),
            };

            // Buffer to handle mismatch between cpal's requested frame count and our BLOCK_SIZE
            let mut remainder_buffer = Vec::with_capacity(config::BLOCK_SIZE * 2);

            AudioDevice::open_default(opts, move |dest| {
                let mut dest_offset = 0;

                // First, drain any remainder from the previous callback
                let remainder_to_copy = remainder_buffer.len().min(dest.len());
                if remainder_to_copy > 0 {
                    dest[..remainder_to_copy]
                        .copy_from_slice(&remainder_buffer[..remainder_to_copy]);
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
                        at_iter(&queue_clone, &mut audio_state, &mut remaining_dest[..block_size_samples]);
                        wp_cloned.signal_audio_tick_complete();
                        remaining_dest = &mut remaining_dest[block_size_samples..];
                    } else {
                        // Not enough space for a full block, generate into our buffer
                        let mut temp_buffer = vec![0.0f32; block_size_samples];
                        at_iter(&queue_clone, &mut audio_state, &mut temp_buffer);
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
            command_queue,
            device: Some(dev),
            worker_pool,
        })
    }

    pub fn batch(&mut self) -> Batch<'_> {
        Batch {
            synthesizer: self,
            commands: Vec::new(),
        }
    }

    /// Convert a duration to time in samples, rounding up.
    pub fn duration_to_samples(&self, dur: Duration) -> usize {
        let s = dur.as_secs_f64();
        let ret = (s * config::SR as f64).ceil() as usize;
        debug_assert!(Duration::from_secs_f64(ret as f64 / config::SR as f64) >= dur);
        ret
    }
}


impl Batch<'_> {
    /// Called on batch creation to catch pending drops from last time, then again on batch publish to catch pending
    /// drops the user might have made during the batch.
    fn handle_pending_drops(&mut self) {
        self.commands.push(Box::new(|state: &mut AudioThreadState| {
            state.mounts.retain(|_, m| {
                !m.pending_drop.load(std::sync::atomic::Ordering::Relaxed)
            });
        }));
    }

    pub fn mount<M: crate::mount_point::Mountable>(&mut self, mut new_mount: M) -> Result<Handle> {
        let mut slots_to_insert = Vec::new();
        
        new_mount.trace(&mut |id, val| match val {
            TracedResource::Slot(s) => slots_to_insert.push((id, s)),
        })?;

        let object_id = UniqueId::new();
        let mount = new_mount.into_mount(self)?;

        let pending_drop = MarkDropped::new();

        let inserting = MountContainer {
            erased_mount: Arc::new(AtomicRefCell::new(mount)),
            pending_drop: pending_drop.0.clone(),
        };
        
        // Create command to insert mount and slots
        let mut inserting_opt = Some(inserting);
        let mut slots_opt = Some(slots_to_insert);
        self.commands.push(Box::new(move |state: &mut AudioThreadState| {
            if let Some(inserting) = inserting_opt.take() {
                state.mounts.insert(object_id, inserting);
            }
            if let Some(slots) = slots_opt.take() {
                for (id, slot) in slots {
                    state.slots.insert(id, slot);
                }
            }
        }));

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
        let slot_id = slot.slot_id;
        let object_id = handle.object_id;
        
        let mut new_val_opt = Some(new_val);
        self.commands.push(Box::new(move |state: &mut AudioThreadState| {
            // Verify the handle exists on the audio thread
            if !state.mounts.contains_key(&object_id) {
                return; // Silent failure for now
            }
            
            if let Some(slot_ref) = state.slots.get_mut(&slot_id) {
                if let Some(container) = slot_ref.downcast_ref::<SlotValueContainer<T>>() {
                    if let Some(new_val) = new_val_opt.take() {
                        let newslot = container.replace(new_val);
                        *slot_ref = Arc::new(newslot);
                    }
                }
            }
        }));

        Ok(())
    }

    /// Convert a source of samples into media in this synthesizer, and return a reference to it, plus a controller to
    /// control the media.
    ///
    /// All media operations happen on a  background thread, and do not flow through the normal command mechanisms.
    ///
    /// All I/O errors are logged on the background thread. The returned media starts paused.
    ///
    /// This is the streaming path to get audio into the library.
    pub fn make_media<S>(
        &mut self,
        source: S,
    ) -> Result<(crate::sample_sources::MediaController, sigs::Media)>
    where
        S: std::io::Read + std::io::Seek + Send + Sync + 'static,
    {
        let unified_source = UnifiedMediaSource::new(source, crate::config::SR as u32)?;
        let descriptor = unified_source.get_descriptor().clone();
        let (task, mut h) = unified_source.into_task_and_handle()?;
        self.synthesizer.worker_pool.register_task(task);

        let ring = h.ring.take();

        Ok((h, sigs::Media { descriptor, ring }))
    }

    pub fn duration_to_samples(&self, dur: Duration) -> usize {
        self.synthesizer.duration_to_samples(dur)
    }
}

/// Run one iteration of the audio thread.
fn at_iter(
    command_queue: &Arc<Mutex<VecDeque<Box<dyn crate::core_traits::Command>>>>,
    state: &mut AudioThreadState,
    dest: &mut [f32],
) {
    // First, execute any pending commands
    {
        let mut queue = command_queue.lock().unwrap();
        while let Some(mut cmd) = queue.pop_front() {
            cmd.execute(state);
        }
    }

    // Then process audio
    at_iter_inner(state, dest);
}

/// Inner implementation of audio thread iteration
fn at_iter_inner(state: &mut AudioThreadState, mut dest: &mut [f32]) {
    while !dest.is_empty() {
        // Copy out whatever data we can from the buffer
        if state.buf_remaining > 0 {
            // Hardcoded stereo, at the moment.
            let remaining = dest.len() / 2;
            let will_do = state.buf_remaining.min(remaining);
            assert!(will_do > 0);

            let start_ind = state.buffer.len() - state.buf_remaining;
            let grabbing = &state.buffer[start_ind..(start_ind + will_do)];
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

        // Prepare the audio thread state for the next block
        state.buf_remaining = config::BLOCK_SIZE;
        // Zero it out for this iteration.
        state.buffer.fill([0.0f64; 2]);

        // Collect mounts that need to run
        let mounts_to_run: Vec<(UniqueId, MountContainer)> = state
            .mounts
            .iter()
            .filter(|(_, m)| !m.pending_drop.load(std::sync::atomic::Ordering::Relaxed))
            .map(|(id, m)| (*id, m.clone()))
            .collect();

        // Process each mount
        for (id, m) in mounts_to_run {
            let slot_ctx = SlotUpdateContext {
                global_slots: &state.slots,
            };
            
            // Create a temporary wrapper for compatibility
            // This will be removed when mounts are updated to work directly with AudioThreadState
            let fake_state = Arc::new(SynthesizerState);
            
            m.erased_mount.borrow_mut().run(
                &fake_state,
                &id,
                &crate::context::FixedSignalExecutionContext {
                    time_in_blocks: state.time_in_blocks,
                    audio_destinationh: atomic_refcell::AtomicRefCell::new(&mut state.buffer),
                    audio_destination_format: &crate::channel_format::ChannelFormat::Stereo,
                    slots: &slot_ctx,
                },
            );
        }

        state.time_in_blocks += 1;
    }
}

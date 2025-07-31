use std::any::Any;
use std::marker::PhantomData as PD;
use std::sync::Arc;
use std::time::Duration;

use arrayvec::ArrayVec;

use crate::config;
use crate::core_traits::*;
use crate::cpal_device::{AudioDevice, DeviceOptions};
use crate::error::Result;
use crate::handle::{Handle, HandleState};
use crate::mark_dropped::MarkDropped;
use crate::program::Program;
use crate::sample_sources::UnifiedMediaSource;
use crate::signals as sigs;
use crate::signals::{Slot, SlotUpdateContext, SlotValueContainer};
use crate::unique_id::UniqueId;

/// Custom recycling strategy for Box<dyn Command>
/// Replaces used commands with no-op closures to maintain allocation
struct CommandRecycler;

impl thingbuf::recycling::Recycle<Box<dyn Command>> for CommandRecycler {
    fn new_element(&self) -> Box<dyn Command> {
        // Create a no-op command
        Box::new(|_: &mut AudioThreadState| {})
    }

    fn recycle(&self, _element: &mut Box<dyn Command>) {
        // Leave it alone. When something is next enqueued here, it will drop the old data on a non-audio thread.
    }
}

pub struct Synthesizer {
    command_ring: Arc<thingbuf::ThingBuf<Box<dyn Command>, CommandRecycler>>,

    device: Option<AudioDevice>,

    worker_pool: crate::worker_pool::WorkerPoolHandle,
}

pub(crate) struct ProgramContainer {
    pub(crate) pending_drop: Arc<std::sync::atomic::AtomicBool>,

    /// Should only be accessed from the audio thread.
    pub(crate) program: Box<Program>,
}

/// Container for slot value and its drop marker
pub(crate) struct SlotContainer {
    pub(crate) value: Arc<dyn Any + Send + Sync + 'static>,
    pub(crate) pending_drop: Arc<std::sync::atomic::AtomicBool>,
}

/// Container for a bus on the audio thread
pub(crate) struct BusContainer {
    pub(crate) bus: Box<dyn crate::bus::GenericBus>,
    pub(crate) pending_drop: Arc<std::sync::atomic::AtomicBool>,
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

    /// Standard library version of programs for audio thread processing
    pub(crate) programs: std::collections::HashMap<UniqueId, ProgramContainer>,

    /// Global slots map for audio thread processing
    pub(crate) slots: std::collections::HashMap<UniqueId, SlotContainer>,

    /// Global buses map for audio thread processing
    pub(crate) buses: std::collections::HashMap<UniqueId, BusContainer>,

    /// Global wavetables map for audio thread processing
    pub(crate) wavetables: std::collections::HashMap<
        UniqueId,
        (
            Arc<crate::wavetable::WaveTable>,
            Arc<std::sync::atomic::AtomicBool>,
        ),
    >,

    /// Reusable vector for collecting programs to run each audio tick
    pub(crate) programs_to_run: Vec<(UniqueId, ProgramContainer)>,

    /// Topological sort data - pre-allocated for efficiency
    pub(crate) topology_generation: u64,
    pub(crate) last_computed_generation: u64,
    pub(crate) program_execution_order: Vec<UniqueId>,
    pub(crate) in_degrees: Vec<(UniqueId, usize)>,
    pub(crate) sort_queue: Vec<UniqueId>,

    /// Program dependency graph (program -> programs it outputs to)
    pub(crate) program_dependencies: std::collections::HashMap<UniqueId, Vec<UniqueId>>,
}

/// A batch of changes for the audio thread.
///
/// You ask for a batch.  Then you manipulate things by asking the batch for their parameters.  The commands all build up,
/// and are stored with the batch.  Then, when the batch drops, they are sent to the audio thread
pub struct Batch<'a> {
    synthesizer: &'a mut Synthesizer,
    commands: Vec<Box<dyn Command>>,
}

impl Drop for Batch<'_> {
    fn drop(&mut self) {
        // Push all commands to the ring buffer, but wrapped under one command so they all apply in the same audio tick.
        // Spin if the buffer is full - audio thread will eventually consume
        let mut outgoing_commands = std::mem::take(&mut self.commands);

        // Create ArrayVecs outside the closure to ensure drops happen off audio thread
        let mut program_ids_to_remove: ArrayVec<UniqueId, 16> = ArrayVec::new();
        let mut slot_ids_to_remove: ArrayVec<UniqueId, 16> = ArrayVec::new();
        let mut bus_ids_to_remove: ArrayVec<UniqueId, 16> = ArrayVec::new();
        let mut wavetable_ids_to_remove: ArrayVec<UniqueId, 16> = ArrayVec::new();
        let mut removed_programs: ArrayVec<ProgramContainer, 16> = ArrayVec::new();
        let mut removed_slots: ArrayVec<SlotContainer, 16> = ArrayVec::new();
        let mut removed_buses: ArrayVec<BusContainer, 16> = ArrayVec::new();
        let mut removed_wavetables: ArrayVec<Arc<crate::wavetable::WaveTable>, 16> =
            ArrayVec::new();

        let mut cmd: Box<dyn Command> = Box::new(move |state: &mut AudioThreadState| {
            // Increment topology generation to trigger recompute
            state.topology_generation += 1;
            // First execute all user commands
            outgoing_commands.iter_mut().for_each(|x| x.execute(state));

            // Then perform cleanup
            // Collect IDs of programs to remove
            program_ids_to_remove.clear();
            program_ids_to_remove.extend(
                state
                    .programs
                    .iter()
                    .filter(|(_, p)| p.pending_drop.load(std::sync::atomic::Ordering::Relaxed))
                    .map(|(id, _)| *id)
                    .take(16),
            );

            // Collect IDs of slots to remove
            slot_ids_to_remove.clear();
            slot_ids_to_remove.extend(
                state
                    .slots
                    .iter()
                    .filter(|(_, s)| s.pending_drop.load(std::sync::atomic::Ordering::Relaxed))
                    .map(|(id, _)| *id)
                    .take(16),
            );

            // Remove programs and store them temporarily
            removed_programs.clear();
            for id in &program_ids_to_remove {
                let program = state
                    .programs
                    .remove(id)
                    .expect("Program marked for removal not found in programs map");
                removed_programs.push(program);
            }

            // Remove slots and store them temporarily
            removed_slots.clear();
            for id in &slot_ids_to_remove {
                let slot = state
                    .slots
                    .remove(id)
                    .expect("Slot marked for removal not found in slots map");
                removed_slots.push(slot);
            }

            // Collect IDs of buses to remove
            bus_ids_to_remove.clear();
            bus_ids_to_remove.extend(
                state
                    .buses
                    .iter()
                    .filter(|(_, b)| b.pending_drop.load(std::sync::atomic::Ordering::Relaxed))
                    .map(|(id, _)| *id)
                    .take(16),
            );

            // Remove buses and store them temporarily
            removed_buses.clear();
            for id in &bus_ids_to_remove {
                let bus = state
                    .buses
                    .remove(id)
                    .expect("Bus marked for removal not found in buses map");
                removed_buses.push(bus);
            }

            // Collect IDs of wavetables to remove
            wavetable_ids_to_remove.clear();
            wavetable_ids_to_remove.extend(
                state
                    .wavetables
                    .iter()
                    .filter(|(_, (_, pending_drop))| {
                        pending_drop.load(std::sync::atomic::Ordering::Relaxed)
                    })
                    .map(|(id, _)| *id)
                    .take(16),
            );

            // Remove wavetables and store them temporarily
            removed_wavetables.clear();
            for id in &wavetable_ids_to_remove {
                let (wavetable, _) = state
                    .wavetables
                    .remove(id)
                    .expect("Wavetable marked for removal not found in wavetables map");
                removed_wavetables.push(wavetable);
            }
        });

        loop {
            match self.synthesizer.command_ring.push(cmd) {
                Ok(()) => break,
                Err(full) => {
                    // Buffer is full, spin and retry
                    // This is safe because audio thread will consume commands
                    std::hint::spin_loop();
                    cmd = full.into_inner();
                }
            }
        }
    }
}

impl Synthesizer {
    pub fn new_default_output() -> Result<Self> {
        let opts = DeviceOptions {
            sample_rate: None, // Let the device choose its preferred sample rate
            channels: Some(2), // Stereo
        };

        // Create a thingbuf ring buffer with reasonable capacity
        // This capacity should be tuned based on expected command rate
        const COMMAND_QUEUE_SIZE: usize = 64;
        let command_ring = Arc::new(thingbuf::ThingBuf::with_recycle(
            COMMAND_QUEUE_SIZE,
            CommandRecycler,
        ));

        let worker_pool =
            crate::worker_pool::WorkerPoolHandle::new_threaded(config::WORKER_POOL_THREADS);

        let dev = {
            let ring_clone = command_ring.clone();
            let wp_cloned = worker_pool.clone();

            // Create persistent audio thread state
            // Pre-allocate reasonable capacity to avoid allocations during audio processing
            let mut audio_state = AudioThreadState {
                buf_remaining: 0,
                buffer: [[0.0f64; 2]; config::BLOCK_SIZE],
                time_in_blocks: 0,
                programs: std::collections::HashMap::with_capacity(256),
                slots: std::collections::HashMap::with_capacity(1024),
                buses: std::collections::HashMap::with_capacity(256),
                wavetables: std::collections::HashMap::with_capacity(256),
                programs_to_run: Vec::with_capacity(256),
                topology_generation: 0,
                last_computed_generation: 0,
                program_execution_order: Vec::with_capacity(256),
                in_degrees: Vec::with_capacity(256),
                sort_queue: Vec::with_capacity(256),
                program_dependencies: std::collections::HashMap::with_capacity(256),
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
                        at_iter(
                            &ring_clone,
                            &mut audio_state,
                            &mut remaining_dest[..block_size_samples],
                        );
                        wp_cloned.signal_audio_tick_complete();
                        remaining_dest = &mut remaining_dest[block_size_samples..];
                    } else {
                        // Not enough space for a full block, generate into our buffer
                        let mut temp_buffer = vec![0.0f32; block_size_samples];
                        at_iter(&ring_clone, &mut audio_state, &mut temp_buffer);
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
            command_ring,
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
    /// Helper to push a command to the batch
    pub(crate) fn push_command<F>(&mut self, f: F)
    where
        F: FnMut(&mut AudioThreadState) + Send + Sync + 'static,
    {
        self.commands.push(Box::new(f));
    }

    pub fn mount(&mut self, program: Program) -> Result<Handle> {
        let object_id = UniqueId::new();

        // Collect all resources that need to be allocated on the audio thread
        let (bus_allocations, wavetable_allocations, input_buses, output_buses) = {
            let state = program.state.read().unwrap();
            let resources = state.resources.lock().unwrap();

            // Collect bus allocations - take from the Mutex<Option<>> if still present
            let mut bus_allocations = Vec::new();
            for (bus_id, state) in &resources.bus_handles {
                if let Some(container) = state.container.lock().unwrap().take() {
                    bus_allocations.push((*bus_id, container));
                }
            }

            // Collect wavetable allocations
            let mut wavetable_allocations = Vec::new();
            for (wavetable_id, state) in &resources.wavetable_handles {
                if let Some(container) = state.container.lock().unwrap().take() {
                    wavetable_allocations.push((*wavetable_id, container));
                }
            }

            // Extract bus linkage information before dropping locks
            let input_buses = state.input_buses.clone();
            let output_buses = state.output_buses.clone();

            (
                bus_allocations,
                wavetable_allocations,
                input_buses,
                output_buses,
            )
        };

        let mut bus_deps = Vec::with_capacity(1024);

        let pending_drop = MarkDropped::new();

        let inserting = ProgramContainer {
            program: Box::new(program),
            pending_drop: pending_drop.0.clone(),
        };

        // Create command to insert program and update dependencies
        let mut inserting_opt = Some(inserting);
        let mut bus_allocations_opt = Some(bus_allocations);
        let mut wavetable_allocations_opt = Some(wavetable_allocations);

        self.push_command(move |state: &mut AudioThreadState| {
            // First, allocate any buses that haven't been allocated yet
            if let Some(bus_allocations) = bus_allocations_opt.take() {
                for (bus_id, container) in bus_allocations {
                    state.buses.insert(bus_id, container);
                }
            }

            // Allocate wavetables
            if let Some(wavetable_allocations) = wavetable_allocations_opt.take() {
                for (wavetable_id, (wavetable, pending_drop)) in wavetable_allocations {
                    state
                        .wavetables
                        .insert(wavetable_id, (wavetable, pending_drop));
                }
            }

            let inserting = inserting_opt.take().unwrap();
            state.programs.insert(object_id, inserting);

            // Update program dependencies based on bus linkages

            // For each output bus, find programs that read from it
            for bus_id in output_buses.keys() {
                for (&other_id, other_container) in &state.programs {
                    if other_id == object_id {
                        continue;
                    }
                    let other_program = &other_container.program;
                    let other_state = other_program.state.read().unwrap();
                    // Check if other program has this bus as input
                    if other_state.input_buses.contains_key(bus_id) {
                        bus_deps.push(other_id);
                    }
                }
            }

            if !bus_deps.is_empty() {
                state
                    .program_dependencies
                    .insert(object_id, std::mem::take(&mut bus_deps));
            }

            // For each input bus, find programs that write to it and add ourselves as their dependency
            for bus_id in input_buses.keys() {
                for (&other_id, other_container) in &state.programs {
                    if other_id == object_id {
                        continue;
                    }
                    let other_program = &other_container.program;
                    let other_state = other_program.state.read().unwrap();
                    // Check if other program has this bus as output
                    if other_state.output_buses.contains_key(bus_id) {
                        state
                            .program_dependencies
                            .entry(other_id)
                            .or_default()
                            .push(object_id);
                    }
                }
            }
        });

        Ok(Handle {
            object_id,
            mark_drop: Arc::new(pending_drop),
            state: Arc::new(std::sync::Mutex::new(HandleState::new())),
        })
    }

    /// Allocate a slot with an initial value.
    ///
    /// The slot is immediately registered in the audio thread's global slots map.
    pub fn allocate_slot<T>(&mut self, initial_value: T) -> Slot<T>
    where
        T: Send + Sync + Clone + 'static,
    {
        let slot_id = UniqueId::new();
        let mark_drop = MarkDropped::new();

        let container = SlotValueContainer::new(initial_value);

        let slot_arc: Arc<dyn Any + Send + Sync + 'static> = Arc::new(container);
        let slot_container = SlotContainer {
            value: slot_arc,
            pending_drop: mark_drop.0.clone(),
        };
        let mut slot_opt = Some(slot_container);

        self.push_command(move |state: &mut AudioThreadState| {
            if let Some(slot) = slot_opt.take() {
                state.slots.insert(slot_id, slot);
            }
        });

        Slot {
            slot_id,
            mark_drop: Arc::new(mark_drop),
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
        // Register the slot with the handle
        let mut state = handle.state.lock().expect("Handle mutex poisoned");
        state.slots.insert(slot.slot_id, slot.mark_drop.clone());

        let slot_id = slot.slot_id;
        let object_id = handle.object_id;

        let mut new_val_opt = Some(new_val);
        self.push_command(move |state: &mut AudioThreadState| {
            // Verify the handle exists on the audio thread
            if !state.programs.contains_key(&object_id) {
                return; // Silent failure for now
            }

            if let Some(slot_container) = state.slots.get_mut(&slot_id) {
                if let Some(container) =
                    slot_container.value.downcast_ref::<SlotValueContainer<T>>()
                {
                    if let Some(new_val) = new_val_opt.take() {
                        let newslot = container.replace(new_val);
                        slot_container.value = Arc::new(newslot);
                    }
                }
            }
        });

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

    /// Create a new bus of the given type
    pub fn create_bus<T: Default + Copy + Send + Sync + 'static>(
        &mut self,
    ) -> crate::bus::BusHandle<T> {
        // Create the handle with lazy allocation
        // The bus will be allocated on the audio thread when a program that uses it is mounted
        crate::bus::BusHandle::new()
    }

    pub fn duration_to_samples(&self, dur: Duration) -> usize {
        self.synthesizer.duration_to_samples(dur)
    }
}

/// Run one iteration of the audio thread.
fn at_iter(
    command_ring: &Arc<thingbuf::ThingBuf<Box<dyn Command>, CommandRecycler>>,
    state: &mut AudioThreadState,
    dest: &mut [f32],
) {
    // First, execute any pending commands
    // pop_ref is lock-free and will not block the audio thread
    while let Some(mut cmd_ref) = command_ring.pop_ref() {
        cmd_ref.execute(state);
        // When cmd_ref is dropped, the recycler will replace it with a no-op
        // This happens in the ring buffer slot, so deallocation is deferred
    }

    // Then process audio
    at_iter_inner(state, dest);
}

/// Compute topological sort of programs based on bus dependencies
/// Uses Kahn's algorithm with pre-allocated buffers
fn compute_program_topology(state: &mut AudioThreadState) {
    if state.last_computed_generation == state.topology_generation {
        return; // Already up to date
    }

    // Clear reusable buffers
    state.program_execution_order.clear();
    state.in_degrees.clear();
    state.sort_queue.clear();

    // Initialize in-degrees for all programs
    for &program_id in state.programs.keys() {
        state.in_degrees.push((program_id, 0));
    }

    // Count in-degrees based on dependencies
    for (&_program_id, deps) in &state.program_dependencies {
        for &dependent_id in deps {
            // Find and increment in-degree
            if let Some(entry) = state
                .in_degrees
                .iter_mut()
                .find(|(id, _)| *id == dependent_id)
            {
                entry.1 += 1;
            }
        }
    }

    // Find all programs with in-degree 0
    for &(id, degree) in &state.in_degrees {
        if degree == 0 {
            state.sort_queue.push(id);
        }
    }

    // Process queue
    while let Some(current_id) = state.sort_queue.pop() {
        state.program_execution_order.push(current_id);

        // Decrement in-degrees of dependent programs
        if let Some(deps) = state.program_dependencies.get(&current_id) {
            for &dep_id in deps {
                if let Some(entry) = state.in_degrees.iter_mut().find(|(id, _)| *id == dep_id) {
                    entry.1 -= 1;
                    if entry.1 == 0 {
                        state.sort_queue.push(dep_id);
                    }
                }
            }
        }
    }

    // Check for cycles
    if state.program_execution_order.len() != state.programs.len() {
        panic!("Cycle detected in program dependencies!");
    }

    state.last_computed_generation = state.topology_generation;
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

        // Reset all buses to their default values
        for bus_container in state.buses.values() {
            bus_container.bus.reset();
        }

        // Compute topological sort if needed
        compute_program_topology(state);

        // Process programs in topologically sorted order
        for &program_id in &state.program_execution_order.clone() {
            if let Some(p) = state.programs.get(&program_id) {
                if p.pending_drop.load(std::sync::atomic::Ordering::Relaxed) {
                    continue;
                }

                // p is already a reference, no need to clone
                let slot_ctx = SlotUpdateContext {
                    global_slots: &state.slots,
                };

                p.program.execute_block(
                    &program_id,
                    &crate::context::FixedSignalExecutionContext {
                        time_in_blocks: state.time_in_blocks,
                        audio_destinationh: atomic_refcell::AtomicRefCell::new(&mut state.buffer),
                        audio_destination_format: &crate::channel_format::ChannelFormat::Stereo,
                        slots: &slot_ctx,
                        buses: &mut state.buses,
                    },
                );
            }
        }

        state.time_in_blocks += 1;
    }
}

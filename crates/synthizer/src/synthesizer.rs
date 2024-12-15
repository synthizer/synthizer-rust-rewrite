use std::marker::PhantomData as PD;
use std::sync::Arc;

use arc_swap::{ArcSwap, ArcSwapOption};
use atomic_refcell::AtomicRefCell;
use rpds::{HashTrieMapSync, VectorSync};

use crate::chain::Chain;
use crate::config;
use crate::core_traits::*;
use crate::mount_point::ErasedMountPoint;
use crate::unique_id::UniqueId;

type SynthMap<K, V> = HashTrieMapSync<K, V>;
type SynthVec<T> = VectorSync<T>;

/// TODO: this is actually private-ish, but we're getting off the ground.
pub struct Synthesizer {
    published_state: Arc<ArcSwap<SynthesizerState>>,
}

#[derive(Clone)]
struct MountContainer {
    pending_drop: Arc<std::sync::atomic::AtomicBool>,
    slots: SynthMap<UniqueId, Arc<AtomicRefCell<Box<dyn std::any::Any + Send + Sync>>>>,
    erased_mount: Arc<AtomicRefCell<Box<dyn ErasedMountPoint>>>,
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
    older_state: Arc<ArcSwapOption<Self>>,

    /// The value is an Arc to an interior-mutable erased box.
    mounts: SynthMap<UniqueId, MountContainer>,

    audio_thred_state: Arc<AtomicRefCell<AudioThreadState>>,
}

/// Ephemeral state for the audio thread itself.  Owned by the audio thread but behind AtomicRefCell to avoid unsafe
/// code.
pub(crate) struct AudioThreadState {
    /// Intermediate mono buffer before going to miniaudio.
    ///
    /// This will be more complex a bit later on. At the moment, it's more "get us off the ground" stuff.
    pub(crate) buffer: [f64; config::BLOCK_SIZE],

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

/// A handle which may be used to manipulate some object of type `T`.
///
/// Handles keep objects alive.  When the last handle drops, the object does as well.
///
/// Handles may alias the same object.  The `T` here is allowing you to manipulate some parameter of your signal in some
/// fashion, but signals may have different parameters and "knobs" all of which may have a different handle (TODO:
/// implement `Slot<T>` stuff).
pub struct Handle<T> {
    object_id: UniqueId,
    mark_drop: Arc<MarkDropped>,
    _phantom: PD<T>,
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self {
            object_id: self.object_id,
            mark_drop: self.mark_drop.clone(),
            _phantom: PD,
        }
    }
}

/// A marker type used as a type parameters to handles which are for an entire mount point.
pub struct MountPointHandleMarker;

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
            .published_state
            .store(Arc::new(self.new_state.clone()));
    }
}

impl Synthesizer {
    pub fn new_audio_defaults() -> Self {
        Self {
            published_state: Arc::new(ArcSwap::new(Arc::new(SynthesizerState::new()))),
        }
    }

    pub fn batch(&mut self) -> Batch<'_> {
        let new_state = Arc::unwrap_or_clone(self.published_state.load_full());

        let mut ret = Batch {
            synthesizer: self,
            new_state,
        };

        // Clear the state out.
        ret.new_state.older_state = Arc::new(ArcSwapOption::new(None));
        ret.handle_pending_drops();

        ret
    }
}

impl SynthesizerState {
    fn new() -> Self {
        Self {
            audio_thred_state: Arc::new(AtomicRefCell::new(AudioThreadState {
                buf_remaining: 0,
                buffer: [0.0f64; config::BLOCK_SIZE],
                time_in_blocks: 0,
            })),
            mounts: SynthMap::new_sync(),
            older_state: Arc::new(ArcSwapOption::new(None)),
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

    pub fn mount<S: IntoSignal>(&mut self, _chain: Chain<S>) -> Handle<MountPointHandleMarker>
    where
        S::Signal: Mountable,
        SignalSealedState<S::Signal>: Send + Sync + 'static,
        SignalSealedParameters<S::Signal>: Send + Sync + 'static,
    {
        let object_id = UniqueId::new();
        let mark_drop = Arc::new(MarkDropped::new());
        Handle {
            object_id,
            mark_drop,
            _phantom: PD,
        }
    }
}

/// Run one iteration of the audio thread.
fn at_iter(state: Arc<SynthesizerState>, mut dest: &mut [f64]) {
    while !dest.is_empty() {
        // Grab the audio thread state and copy out whatever data we can.
        {
            let mut state = state.audio_thred_state.borrow_mut();
            if state.buf_remaining > 0 {
                let remaining = dest.len();
                let will_do = state.buf_remaining.min(remaining);
                assert!(will_do > 0);

                let start_ind = state.buffer.len() - state.buf_remaining;
                let grabbing = &mut state.buffer[start_ind..(start_ind + will_do)];
                (dest[..will_do]).copy_from_slice(grabbing);
                dest = &mut dest[will_do..];

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
            at_state.buffer.fill(0.0f64);
        }

        // Mounts may fill the audio buffer.
        for (_, m) in state.mounts.iter() {
            if m.pending_drop.load(std::sync::atomic::Ordering::Relaxed) {
                continue;
            }

            m.erased_mount.borrow_mut().run(&state);
        }

        state.audio_thred_state.borrow_mut().time_in_blocks += 1;
    }
}

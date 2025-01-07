use std::any::Any;
use std::marker::PhantomData as PD;
use std::sync::Arc;

use rpds::HashTrieMapSync;

use crate::core_traits::*;
use crate::error::Result;
use crate::unique_id::UniqueId;

pub(crate) type SlotMap<K, V> = HashTrieMapSync<K, V>;

/// Reference to a "knob" on your algorithm.
///
/// In order to get information to the audio thread, you must have a way to communicate with it.  Slots solve that
/// problem.  You may resolve a slot against a synthesizer and a mount handle, and then replace or (in some cases)
/// mutate the value.
///
/// Mutation is only available if `T` is Clone.  This is because the library is internally using persistent data
/// structures.  In some but not all cases, therefore, a clone happens first.  In general this happens for the first
/// access in a batch, so it is still worthwhile to use mutation if you can.
///
/// Slots are unique and tied to their mounts.  If you mix up a mount handle and a slot for that mount, an error
/// results.  You cannot use a slot without a mount handle.
///
/// Slots are one way.  You can't send data out from the audio thread.  You can read the last set value but that's it.
/// Note that reading the value outside the audio thread is slow, on the order of a couple tree traversals and some
/// dynamic casting, and it may get slower in the future.  All optimization effort goes toward writing values.
///
/// Slots are signals in the same way that scalars are, and so you can then pass the slot to `Chain::new`.  Behavior is
/// undefined if you use a slot with more than one mount at a time, but it is fine to use the slot multiple times in one
/// given mount.
///
/// If you mix up slots on the audio thread such that a chain is mounted in a different mount than the chain from which
/// you got the slot, mounting will error.
///
/// You get a slot from a batch.  The slot outlives the batch without a problem, but do note that creation is expensive
/// if you create only one slot at a time.
pub struct Slot<T> {
    pub(crate) slot_id: UniqueId,
    pub(crate) _phantom: PD<T>,
}

/// Internal state for a slot's value.
///
/// These get a unique id for the update, plus the value.  When mutating, the unique id is changed if needed.  This can
/// then be used to intelligently detect changes on the audio thread.  What actually happens is that we introduce one
/// more level of redirection.  `Arc` can tell us whether or not any given value made it to the audio thread; if it
/// didn't we can avoid allocating more ids.
#[derive(Clone)]
pub(crate) struct SlotValueContainer<T> {
    value: Arc<T>,
    update_id: UniqueId,
}

/// The audio thread state of a slot.
///
/// This can tell you the block at which a change last happened in addition to the value.
///
/// The signal resolves these at block start.
///
/// This is also the state for the signal: each signal is one slot, and so can directly hold this without having to use
/// erasure.
pub struct SlotAudioThreadState<T> {
    slot_id: UniqueId,

    /// Same `Arc` as the SlotValueContainer.
    value: Arc<T>,
    last_update_id: UniqueId,

    /// Set when a new id is found. Cleared on the next `on_block_start` call.
    changed_this_block: bool,
}

/// Subset of the synthesizer's state needed to resolve a slot.
pub(crate) struct SlotUpdateContext<'a> {
    /// Slots on this mount.
    ///
    /// The value is `SlotValueContainer<T>`.
    pub(crate) mount_slots: &'a SlotMap<UniqueId, Arc<dyn Any + Send + Sync + 'static>>,
}

impl SlotUpdateContext<'_> {
    fn resolve<T: Any>(&'_ self, id: &'_ UniqueId) -> Option<&'_ SlotValueContainer<T>> {
        self.mount_slots
            .get(id)?
            .downcast_ref::<SlotValueContainer<T>>()
    }
}

pub struct SlotSignalConfig<T> {
    slot_id: UniqueId,

    initial_value: T,
}

pub struct SlotSignal<T>(PD<T>);

/// Output from a slot's signal.
///
/// Every slot is reading from a fixed place in memory, but it has other metadata it wishes to hand out, primarily
/// change notifications.
#[derive(Clone)] // TODO: we can lift the Clone requirement in a bit.
pub struct SlotSignalOutput<T> {
    value: T,
    changed_this_block: bool,
}

impl<T> SlotSignalOutput<T> {
    pub fn get_value(&self) -> &T {
        &self.value
    }

    /// Did the value change at the beginning of this block?
    ///
    /// Note that block intervals are subject to change. Primarily this is useful for crossfades or one-off triggers of
    /// other logic.
    pub fn changed_this_block(&self) -> bool {
        self.changed_this_block
    }
}

unsafe impl<T: Send + Sync + 'static + Clone> Signal for SlotSignal<T>
where
    T: Clone,
{
    type Input<'il> = ();
    type Output<'ol> = SlotSignalOutput<T>;
    type State = SlotAudioThreadState<T>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        let slot_container = ctx
            .fixed
            .slots
            .resolve::<T>(&state.slot_id)
            .expect("If a slot is created, it should have previously been allocated");
        let is_change = state.last_update_id != slot_container.update_id;

        // If no change has occurred, optimize out doing anything.
        if !is_change {
            state.changed_this_block = false;
            return;
        }

        state.value = slot_container.value.clone();
        state.last_update_id = slot_container.update_id;
        state.changed_this_block = true;
    }

    fn tick<'il, 'ol, 's, I, const N: usize>(
        _ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        _input: I,
        state: &'s mut Self::State,
    ) -> impl ValueProvider<Self::Output<'ol>>
    where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        's: 'ol,
        I: ValueProvider<Self::Input<'il>> + Sized,
    {
        ClosureProvider::<_, _, N>::new(|_| SlotSignalOutput {
            value: (*state.value).clone(),
            changed_this_block: state.changed_this_block,
        })
    }
}

impl<T> IntoSignal for SlotSignalConfig<T>
where
    T: 'static + Send + Sync + Clone,
{
    type Signal = SlotSignal<T>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        Ok(ReadySignal {
            signal: SlotSignal(PD),

            state: SlotAudioThreadState {
                slot_id: self.slot_id,
                changed_this_block: false,
                last_update_id: UniqueId::new(),
                value: Arc::new(self.initial_value),
            },
        })
    }

    fn trace<F: FnMut(UniqueId, TracedResource)>(&mut self, inserter: &mut F) -> Result<()> {
        let ns = SlotValueContainer {
            update_id: UniqueId::new(),
            value: Arc::new(self.initial_value.clone()),
        };

        inserter(self.slot_id, TracedResource::Slot(Arc::new(ns)));
        Ok(())
    }
}

impl<T> Slot<T>
where
    T: Send + Sync + Clone + 'static,
{
    /// Get a signal which will read this slot.
    pub(crate) fn read_signal(
        &self,
        initial_value: T,
    ) -> impl IntoSignal<Signal = impl for<'a> Signal<Input<'a> = (), Output<'a> = T>> {
        crate::signals::MapSignalConfig::new(
            SlotSignalConfig {
                initial_value,
                slot_id: self.slot_id,
            },
            |x: SlotSignalOutput<T>| -> T { x.value.clone() },
        )
    }

    /// Get a signal which will read this slot, then tack on a boolean indicating whether the value changed this block.
    pub(crate) fn read_signal_and_change_flag(
        &self,
        initial_value: T,
    ) -> impl IntoSignal<Signal = impl for<'a> Signal<Input<'a> = (), Output<'a> = (T, bool)>> {
        crate::signals::MapSignalConfig::new(
            SlotSignalConfig {
                initial_value,
                slot_id: self.slot_id,
            },
            |x: SlotSignalOutput<T>| -> (T, bool) { (x.value.clone(), x.changed_this_block) },
        )
    }
}

impl<T> SlotValueContainer<T> {
    #[must_use = "This is an immutable type in a persistent data structure"]
    pub(crate) fn replace(&self, newval: T) -> Self {
        Self {
            value: Arc::new(newval),
            update_id: UniqueId::new(),
        }
    }
}

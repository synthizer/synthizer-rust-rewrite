use crate::bus::{BusHandle, BusLink, BusLinkType};
use crate::config;
use crate::context::{FixedSignalExecutionContext, SignalExecutionContext};
use crate::core_traits::{IntoSignal, Signal, SignalState};
use crate::error::Result;
use crate::unique_id::UniqueId;
use crate::Chain;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Resources tracked by a program during chain construction
#[derive(Default)]
pub(crate) struct ProgramResources {
    pub(crate) bus_handles: Vec<(UniqueId, Arc<crate::bus::BusHandleState>)>,
    pub(crate) slots: HashSet<UniqueId>,
    pub(crate) wavetable_handles: Vec<(UniqueId, Arc<crate::wavetable::WaveTableHandleState>)>,
}

/// Internal state of a program wrapped in RwLock for interior mutability
pub(crate) struct ProgramState {
    pub(crate) fragments: Vec<Box<dyn ProgramFragment>>,
    pub(crate) resources: Arc<Mutex<ProgramResources>>,
    pub(crate) input_buses: HashMap<UniqueId, Vec<UniqueId>>,
    pub(crate) output_buses: HashMap<UniqueId, Vec<UniqueId>>,
    pub(crate) internal_buses: Vec<UniqueId>,
}

/// A program is a collection of signal fragments that execute together as a unit.
///
/// Programs are the primary unit of execution in the synthesizer. Each program can contain multiple fragments, where
/// each fragment processes audio independently. All fragments in a program share the same execution context and run
/// sequentially.
pub struct Program {
    /// Unique ID for this program
    pub(crate) id: UniqueId,

    /// Internal state wrapped in RefCell for interior mutability
    pub(crate) state: RefCell<ProgramState>,
}

impl Program {
    /// Create a new empty program.
    pub fn new() -> Self {
        Self {
            id: UniqueId::new(),
            state: RefCell::new(ProgramState {
                fragments: Vec::new(),
                resources: Arc::new(Mutex::new(ProgramResources::default())),
                input_buses: HashMap::new(),
                output_buses: HashMap::new(),
                internal_buses: Vec::new(),
            }),
        }
    }

    /// Create a new empty chain for this program
    pub fn new_chain(&self) -> Chain<'_, crate::chain::EmptyChain> {
        Chain::new(self)
    }

    /// Add a signal fragment to this program.
    ///
    /// The signal must have no input (Input = ()) but can have any output type.
    /// The output is discarded as fragments run independently.
    pub fn add_fragment<S, O>(&self, signal_config: Chain<'_, S>) -> Result<()>
    where
        S: IntoSignal,
        S::Signal: Signal<Input = (), Output = O> + 'static,
        O: 'static,
    {
        let ready = signal_config.inner.into_signal()?;

        let fragment = SignalFragment {
            signal: ready.signal,
            state: ready.state,
        };

        self.state.borrow_mut().fragments.push(Box::new(fragment));
        Ok(())
    }

    /// Execute one block of audio for all fragments in this program.
    pub(crate) fn execute_block(
        &self,
        program_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    ) {
        let mut state = self.state.borrow_mut();
        for fragment in &mut state.fragments {
            fragment.execute_block(program_id, shared_ctx);
        }
    }

    /// Link an input bus to this program
    pub fn link_input_bus<'a, T>(&'a self, bus: &BusHandle<T>) -> BusLink<'a, T> {
        let mut state = self.state.borrow_mut();
        state.input_buses.insert(bus.id(), Vec::new());
        state
            .resources
            .lock()
            .unwrap()
            .bus_handles
            .push((bus.id(), bus.state.clone()));
        BusLink {
            program: self,
            bus_id: bus.id(),
            link_type: BusLinkType::Input,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Link an output bus to this program
    pub fn link_output_bus<'a, T>(&'a self, bus: &BusHandle<T>) -> BusLink<'a, T> {
        let mut state = self.state.borrow_mut();
        state.output_buses.insert(bus.id(), Vec::new());
        state
            .resources
            .lock()
            .unwrap()
            .bus_handles
            .push((bus.id(), bus.state.clone()));
        BusLink {
            program: self,
            bus_id: bus.id(),
            link_type: BusLinkType::Output,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Link an internal bus to this program
    pub fn link_internal_bus<'a, T>(&'a self, bus: &BusHandle<T>) -> BusLink<'a, T> {
        let mut state = self.state.borrow_mut();
        state.internal_buses.push(bus.id());
        state
            .resources
            .lock()
            .unwrap()
            .bus_handles
            .push((bus.id(), bus.state.clone()));
        BusLink {
            program: self,
            bus_id: bus.id(),
            link_type: BusLinkType::Internal,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Track that this program uses a slot
    pub fn uses_slot<T>(&self, slot: &crate::signals::Slot<T>) {
        self.state
            .borrow_mut()
            .resources
            .lock()
            .unwrap()
            .slots
            .insert(slot.slot_id);
    }

    /// Track that this program uses a wavetable
    pub fn uses_wavetable(&self, wavetable: &crate::wavetable::WaveTableHandle) {
        self.state
            .borrow_mut()
            .resources
            .lock()
            .unwrap()
            .wavetable_handles
            .push((wavetable.id(), wavetable.state.clone()));
    }

    /// Create a chain that reads from a wavetable with truncation
    pub fn chain_wavetable_truncated<'p, F, const LOOPING: bool>(
        &'p self,
        wavetable: &crate::wavetable::WaveTableHandle,
        increment: f64,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = (), Output = F>>>
    where
        F: crate::core_traits::AudioFrame<f64> + Send + Sync + 'static,
    {
        self.state
            .borrow_mut()
            .resources
            .lock()
            .unwrap()
            .wavetable_handles
            .push((wavetable.id(), wavetable.state.clone()));
        Chain::<crate::chain::EmptyChain>::with(
            wavetable.read_truncated::<F, LOOPING>(increment),
            self,
        )
    }

    /// Create a chain that reads from a wavetable with linear interpolation
    pub fn chain_wavetable_linear<'p, F, const LOOPING: bool>(
        &'p self,
        wavetable: &crate::wavetable::WaveTableHandle,
        increment: f64,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = (), Output = F>>>
    where
        F: crate::core_traits::AudioFrame<f64> + Send + Sync + 'static,
    {
        self.state
            .borrow_mut()
            .resources
            .lock()
            .unwrap()
            .wavetable_handles
            .push((wavetable.id(), wavetable.state.clone()));
        Chain::<crate::chain::EmptyChain>::with(
            wavetable.read_linear::<F, LOOPING>(increment),
            self,
        )
    }

    /// Create a chain that reads from a wavetable with cubic interpolation
    pub fn chain_wavetable_cubic<'p, F, const LOOPING: bool>(
        &'p self,
        wavetable: &crate::wavetable::WaveTableHandle,
        increment: f64,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = (), Output = F>>>
    where
        F: crate::core_traits::AudioFrame<f64> + Send + Sync + 'static,
    {
        self.state
            .borrow_mut()
            .resources
            .lock()
            .unwrap()
            .wavetable_handles
            .push((wavetable.id(), wavetable.state.clone()));
        Chain::<crate::chain::EmptyChain>::with(wavetable.read_cubic::<F, LOOPING>(increment), self)
    }

    /// Create a chain from media
    pub fn chain_media<'p, const MAX_CHANS: usize>(
        &'p self,
        media: &mut crate::signals::Media,
        wanted_format: crate::channel_format::ChannelFormat,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = (), Output = [f64; MAX_CHANS]>>>
    {
        Chain::<crate::chain::EmptyChain>::with(media.into_config(wanted_format), self)
    }
}

/// Trait for program fragments that can be executed as part of a program.
///
/// This is the type-erased interface that allows different types of fragments to be stored and executed together.
pub(crate) trait ProgramFragment: Send + 'static {
    /// Execute one block of audio processing for this fragment.
    fn execute_block(&mut self, program_id: &UniqueId, shared_ctx: &FixedSignalExecutionContext);
}

/// A program fragment that wraps a signal and its state.
///
/// This is the concrete implementation that allows signals to be used as fragments.
struct SignalFragment<S>
where
    S: Signal<Input = ()>,
{
    signal: S,
    state: SignalState<S>,
}

impl<S> ProgramFragment for SignalFragment<S>
where
    S: Signal<Input = ()>,
{
    fn execute_block(&mut self, _program_id: &UniqueId, shared_ctx: &FixedSignalExecutionContext) {
        let ctx = SignalExecutionContext { fixed: shared_ctx };

        // Call on_block_start once per block
        S::on_block_start(&ctx, &mut self.state);

        // Process each frame in the block
        for _ in 0..config::BLOCK_SIZE {
            // Discard the output
            let _ = S::tick_frame(&ctx, (), &mut self.state);
        }
    }
}

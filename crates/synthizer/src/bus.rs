use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::sync::Arc;

use crate::config;
use crate::core_traits::{AudioFrame, IntoSignal, IntoSignalInput, Signal};
use crate::mark_dropped::MarkDropped;
use crate::unique_id::UniqueId;

/// Type-erased trait for buses
pub(crate) trait GenericBus: Send + Sync {
    /// Reset the bus to its default value
    fn reset(&self);

    /// Get the unique ID of this bus
    fn id(&self) -> UniqueId;

    /// Get a reference to self as Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// A bus is a block-sized buffer for inter-program audio communication.
///
/// Buses are global resources that allow programs to share audio data. They support reading, writing, and binary
/// operations.
///
/// IMPORTANT: Buses are block-wise.  That is, if you recurse into the bus in the same program you'll get data in an
/// undefined but unsafe manner because the signals advance and each contain their own counter.
pub(crate) struct Bus<T> {
    /// The actual buffer. We use UnsafeCell because we need to hand out
    /// raw pointers during on_block_start for performance.
    buffer: UnsafeCell<Box<[T; config::BLOCK_SIZE]>>,

    /// Unique identifier for this bus
    id: UniqueId,
}

// Safety: Bus is Send + Sync if T is Send + Sync
// We ensure exclusive access through the audio thread's single-threaded execution
unsafe impl<T: Send + Sync> Send for Bus<T> {}
unsafe impl<T: Send + Sync> Sync for Bus<T> {}

impl<T: Default + Copy + Send + Sync + 'static> GenericBus for Bus<T> {
    fn reset(&self) {
        unsafe {
            let buffer = &mut **self.buffer.get();
            buffer.fill(T::default());
        }
    }

    fn id(&self) -> UniqueId {
        self.id
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl<T> Bus<T> {
    /// Get a raw pointer to the buffer.
    ///
    /// # Safety
    /// This is only safe to call from the audio thread during on_block_start.
    /// The pointer is valid for the duration of the current block only.
    pub(crate) unsafe fn as_mut_ptr(&self) -> *mut [T; config::BLOCK_SIZE] {
        &mut **self.buffer.get()
    }
}

impl<T: Default + Copy> Bus<T> {
    /// Create a new bus with a specific ID
    pub(crate) fn new_with_id(id: UniqueId) -> Self {
        Self {
            buffer: UnsafeCell::new(Box::new([T::default(); config::BLOCK_SIZE])),
            id,
        }
    }
}

/// A handle to a bus that can be used to link it to programs
pub struct BusHandle<T> {
    pub(crate) bus_id: UniqueId,
    pub(crate) mark_drop: Arc<MarkDropped>,
    pub(crate) _phantom: PhantomData<T>,
}

impl<T> BusHandle<T> {
    /// Get the ID of this bus
    pub(crate) fn id(&self) -> UniqueId {
        self.bus_id
    }
}

/// Type of bus linkage to a program
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusLinkType {
    /// Bus provides input to the program
    Input,
    /// Program writes output to the bus
    Output,
    /// Bus is used internally by the program only
    Internal,
}

/// A link between a bus and a program.
///
/// This type is used during program construction to establish bus connections with a fluent API.
pub struct BusLink<'a, T> {
    pub(crate) program: &'a mut crate::program::Program,
    pub(crate) bus_id: UniqueId,
    pub(crate) link_type: BusLinkType,
    pub(crate) _phantom: PhantomData<T>,
}

impl<T> BusLink<'_, T> {
    /// Create a chain that reads from this bus
    pub fn read(self) -> crate::Chain<impl IntoSignal<Signal = impl Signal<Input = (), Output = T>>>
    where
        T: Send + Sync + Copy + Default + 'static,
    {
        crate::Chain::new(ReadBusSignalConfig {
            bus_id: self.bus_id,
            _phantom: PhantomData::<T>,
        })
    }

    /// Write a signal chain to this bus
    pub fn write<S>(
        self,
        chain: crate::Chain<S>,
    ) -> crate::Chain<impl IntoSignal<Signal = impl Signal<Input = (), Output = ()>>>
    where
        S: IntoSignal + 'static,
        S::Signal: Signal<Input = (), Output = T>,
        T: Send + Sync + Copy + 'static,
    {
        crate::Chain {
            inner: crate::signals::AndThenConfig {
                left: chain.inner,
                right: WriteBusSignalConfig {
                    bus_id: self.bus_id,
                    _phantom: PhantomData,
                },
            },
        }
    }

    /// Add a signal chain's output to this bus element-wise for AudioFrame types
    pub fn frame_add<S>(
        self,
        chain: crate::Chain<S>,
    ) -> crate::Chain<impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = ()>>>
    where
        S: IntoSignal + 'static,
        S::Signal: Signal<Output = T>,
        T: AudioFrame<f64> + Copy + Send + Sync + 'static,
    {
        crate::Chain {
            inner: crate::signals::AndThenConfig {
                left: chain.inner,
                right: BinOpBusSignalConfig {
                    bus_id: self.bus_id,
                    op: |dst: &mut T, src: T| {
                        let channels = dst.channel_count().min(src.channel_count());
                        for i in 0..channels {
                            dst.set(i, dst.get(i) + src.get(i));
                        }
                    },
                    _phantom: PhantomData,
                },
            },
        }
    }
}

/// Configuration for a signal that writes to a bus
pub struct WriteBusSignalConfig<T> {
    bus_id: UniqueId,
    _phantom: PhantomData<T>,
}

impl<T: Send + Sync + Copy + 'static> IntoSignal for WriteBusSignalConfig<T> {
    type Signal = WriteBusSignal<T>;

    fn into_signal(
        self,
    ) -> crate::error::Result<crate::core_traits::ReadySignal<Self::Signal, WriteBusSignalState<T>>>
    {
        Ok(crate::core_traits::ReadySignal {
            signal: WriteBusSignal {
                _phantom: PhantomData,
            },
            state: WriteBusSignalState {
                bus_id: self.bus_id,
                bus_ptr: std::ptr::null_mut(), // Will be set in on_block_start
                position: 0,
            },
        })
    }
}

/// Signal that writes to a bus
pub struct WriteBusSignal<T> {
    _phantom: PhantomData<T>,
}

/// State for WriteBusSignal
pub struct WriteBusSignalState<T> {
    bus_id: UniqueId,
    bus_ptr: *mut [T; config::BLOCK_SIZE],
    position: usize,
}

// Safety: We only access bus_ptr from the audio thread
unsafe impl<T: Send> Send for WriteBusSignalState<T> {}
unsafe impl<T: Send> Sync for WriteBusSignalState<T> {}

unsafe impl<T: Send + Sync + Copy + 'static> Signal for WriteBusSignal<T> {
    type Input = T;
    type Output = ();
    type State = WriteBusSignalState<T>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        // Reset position for new block
        state.position = 0;

        // Look up the bus and cache its pointer
        if let Some(bus_container) = ctx.fixed.buses.get(&state.bus_id) {
            if let Some(bus) = bus_container.bus.as_any().downcast_ref::<Bus<T>>() {
                state.bus_ptr = unsafe { bus.as_mut_ptr() };
            }
        }
    }

    fn tick_frame(
        _ctx: &crate::context::SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        if state.position < config::BLOCK_SIZE {
            unsafe {
                (*state.bus_ptr)[state.position] = input;
            }
            state.position += 1;
        }
    }
}

/// Configuration for a signal that reads from a bus
pub struct ReadBusSignalConfig<T> {
    bus_id: UniqueId,
    _phantom: PhantomData<T>,
}

impl<T: Send + Sync + Copy + Default + 'static> IntoSignal for ReadBusSignalConfig<T> {
    type Signal = ReadBusSignal<T>;

    fn into_signal(
        self,
    ) -> crate::error::Result<crate::core_traits::ReadySignal<Self::Signal, ReadBusSignalState<T>>>
    {
        Ok(crate::core_traits::ReadySignal {
            signal: ReadBusSignal {
                _phantom: PhantomData,
            },
            state: ReadBusSignalState {
                bus_id: self.bus_id,
                bus_ptr: std::ptr::null_mut(), // Will be set in on_block_start
                position: 0,
            },
        })
    }
}

/// Signal that reads from a bus
pub struct ReadBusSignal<T> {
    _phantom: PhantomData<T>,
}

/// State for ReadBusSignal
pub struct ReadBusSignalState<T> {
    bus_id: UniqueId,
    bus_ptr: *mut [T; config::BLOCK_SIZE],
    position: usize,
}

// Safety: We only access bus_ptr from the audio thread
unsafe impl<T: Send> Send for ReadBusSignalState<T> {}
unsafe impl<T: Send> Sync for ReadBusSignalState<T> {}

unsafe impl<T: Send + Sync + Copy + Default + 'static> Signal for ReadBusSignal<T> {
    type Input = ();
    type Output = T;
    type State = ReadBusSignalState<T>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        // Reset position for new block
        state.position = 0;

        // Look up the bus and cache its pointer
        if let Some(bus_container) = ctx.fixed.buses.get(&state.bus_id) {
            if let Some(bus) = bus_container.bus.as_any().downcast_ref::<Bus<T>>() {
                state.bus_ptr = unsafe { bus.as_mut_ptr() };
            }
        }
    }

    fn tick_frame(
        _ctx: &crate::context::SignalExecutionContext<'_, '_>,
        _input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        if state.position < config::BLOCK_SIZE {
            let value = unsafe { (*state.bus_ptr)[state.position] };
            state.position += 1;
            value
        } else {
            T::default()
        }
    }
}

/// Configuration for a signal that applies a binary operation to a bus
pub struct BinOpBusSignalConfig<T, F> {
    bus_id: UniqueId,
    op: F,
    _phantom: PhantomData<T>,
}

impl<T, F> IntoSignal for BinOpBusSignalConfig<T, F>
where
    T: Send + Sync + Copy + 'static,
    F: FnMut(&mut T, T) + Send + Sync + 'static,
{
    type Signal = BinOpBusSignal<T, F>;

    fn into_signal(
        self,
    ) -> crate::error::Result<
        crate::core_traits::ReadySignal<Self::Signal, BinOpBusSignalState<T, F>>,
    > {
        Ok(crate::core_traits::ReadySignal {
            signal: BinOpBusSignal {
                _phantom: PhantomData,
            },
            state: BinOpBusSignalState {
                bus_id: self.bus_id,
                bus_ptr: std::ptr::null_mut(), // Will be set in on_block_start
                position: 0,
                op: self.op,
            },
        })
    }
}

/// Signal that applies a binary operation to a bus
pub struct BinOpBusSignal<T, F> {
    _phantom: PhantomData<(T, F)>,
}

/// State for BinOpBusSignal
pub struct BinOpBusSignalState<T, F> {
    bus_id: UniqueId,
    bus_ptr: *mut [T; config::BLOCK_SIZE],
    position: usize,
    op: F,
}

// Safety: We only access bus_ptr from the audio thread
unsafe impl<T: Send, F: Send> Send for BinOpBusSignalState<T, F> {}
unsafe impl<T: Send, F: Sync> Sync for BinOpBusSignalState<T, F> {}

unsafe impl<T, F> Signal for BinOpBusSignal<T, F>
where
    T: Send + Sync + Copy + 'static,
    F: FnMut(&mut T, T) + Send + Sync + 'static,
{
    type Input = T;
    type Output = ();
    type State = BinOpBusSignalState<T, F>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        // Reset position for new block
        state.position = 0;

        // Look up the bus and cache its pointer
        if let Some(bus_container) = ctx.fixed.buses.get(&state.bus_id) {
            if let Some(bus) = bus_container.bus.as_any().downcast_ref::<Bus<T>>() {
                state.bus_ptr = unsafe { bus.as_mut_ptr() };
            }
        }
    }

    fn tick_frame(
        _ctx: &crate::context::SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        if state.position < config::BLOCK_SIZE {
            unsafe {
                let dst = &mut (*state.bus_ptr)[state.position];
                (state.op)(dst, input);
            }
            state.position += 1;
        }
    }
}

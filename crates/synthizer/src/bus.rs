use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::ops::Add;

use crate::config;
use crate::core_traits::{AudioFrame, Signal};
use crate::unique_id::UniqueId;

/// A bus is a block-sized buffer for inter-program audio communication.
/// 
/// Buses are global resources that allow programs to share audio data.
/// They support reading, writing, and binary operations.
pub struct Bus<T> {
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

impl<T> Bus<T> {
    /// Get the unique ID of this bus
    pub fn id(&self) -> UniqueId {
        self.id
    }
    
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
    /// Create a new bus with default values
    pub fn new() -> Self {
        Self {
            buffer: UnsafeCell::new(Box::new([T::default(); config::BLOCK_SIZE])),
            id: UniqueId::new(),
        }
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
/// This type is used during program construction to establish bus connections.
pub struct BusLink<'a, T> {
    pub(crate) program: &'a mut crate::program::Program,
    pub(crate) bus_id: UniqueId,
    pub(crate) link_type: BusLinkType,
    pub(crate) _phantom: PhantomData<T>,
}

impl<'a, T> BusLink<'a, T> {
    /// Create a signal that writes to this bus
    pub fn write_bus(&self) -> WriteBusSignalConfig<T> {
        WriteBusSignalConfig {
            bus_id: self.bus_id,
            _phantom: PhantomData,
        }
    }
    
    /// Create a signal that reads from this bus
    pub fn read_bus(&self) -> ReadBusSignalConfig<T> {
        ReadBusSignalConfig {
            bus_id: self.bus_id,
            _phantom: PhantomData,
        }
    }
    
    /// Create a signal that applies a binary operation to the bus
    pub fn binop<F>(&self, op: F) -> BinOpBusSignalConfig<T, F>
    where
        F: FnMut(&mut T, T) + Send + Sync + 'static,
    {
        BinOpBusSignalConfig {
            bus_id: self.bus_id,
            op,
            _phantom: PhantomData,
        }
    }
    
    /// Create a signal that adds to the bus (for T: Add)
    pub fn add(&self) -> BinOpBusSignalConfig<T, impl FnMut(&mut T, T)>
    where
        T: Add<Output = T> + Copy,
    {
        self.binop(|dst, src| *dst = *dst + src)
    }
}

/// Configuration for a signal that writes to a bus
pub struct WriteBusSignalConfig<T> {
    bus_id: UniqueId,
    _phantom: PhantomData<T>,
}

impl<T: Send + Sync + Copy + 'static> crate::core_traits::IntoSignal for WriteBusSignalConfig<T> {
    type Signal = WriteBusSignal<T>;
    
    fn into_signal(self) -> crate::error::Result<crate::core_traits::ReadySignal<Self::Signal, WriteBusSignalState<T>>> {
        Ok(crate::core_traits::ReadySignal {
            signal: WriteBusSignal {
                _phantom: PhantomData,
            },
            state: WriteBusSignalState {
                bus_ptr: std::ptr::null_mut(), // Will be set by the audio thread
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
        _ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        // Reset position for new block
        state.position = 0;
        // Note: bus_ptr is set during signal initialization from the bus registry
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

impl<T: Send + Sync + Copy + Default + 'static> crate::core_traits::IntoSignal for ReadBusSignalConfig<T> {
    type Signal = ReadBusSignal<T>;
    
    fn into_signal(self) -> crate::error::Result<crate::core_traits::ReadySignal<Self::Signal, ReadBusSignalState<T>>> {
        Ok(crate::core_traits::ReadySignal {
            signal: ReadBusSignal {
                _phantom: PhantomData,
            },
            state: ReadBusSignalState {
                bus_ptr: std::ptr::null_mut(), // Will be set by the audio thread
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
        _ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        // Reset position for new block
        state.position = 0;
        // Note: bus_ptr is set during signal initialization from the bus registry
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

impl<T, F> crate::core_traits::IntoSignal for BinOpBusSignalConfig<T, F>
where
    T: Send + Sync + Copy + 'static,
    F: FnMut(&mut T, T) + Send + Sync + 'static,
{
    type Signal = BinOpBusSignal<T, F>;
    
    fn into_signal(self) -> crate::error::Result<crate::core_traits::ReadySignal<Self::Signal, BinOpBusSignalState<T, F>>> {
        Ok(crate::core_traits::ReadySignal {
            signal: BinOpBusSignal {
                _phantom: PhantomData,
            },
            state: BinOpBusSignalState {
                bus_ptr: std::ptr::null_mut(), // Will be set by the audio thread
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
        _ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        // Reset position for new block
        state.position = 0;
        // Note: bus_ptr is set during signal initialization from the bus registry
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

/// Extension trait for AudioFrame to support truncated addition
pub trait AudioFrameExt: AudioFrame<f64> {
    /// Add only the first N channels
    fn add_truncate<const CHANS: usize>(&mut self, other: &Self) {
        for i in 0..CHANS.min(self.channel_count()) {
            let sum = self.get(i) + other.get(i);
            self.set(i, sum);
        }
    }
}
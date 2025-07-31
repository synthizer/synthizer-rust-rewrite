#![allow(clippy::type_complexity)]
use crate::channel_format::ChannelFormat;
use crate::config;
use crate::core_traits::*;
use crate::signals as sigs;

/// The main entrypoint to signal building.
///
/// You use the various methods on the chain to build up signal types, with their associated config.  Chains may have
/// other chains embedded in them, which allows recursion.  Math ops work on chains with compatible outputs: `a + b` is
/// valid, if a and b produce f64 for example.  Chains are also themselves signals (this is how you get a `Signal`
/// implementor).  Note that the only other kind of signal which is public is constant values of various forms: you
/// don't have to make a chain to have a constant inside a chain, just use the constant.
///
/// Chains have inputs and outputs.  This is why chains can be put into other chains: you may feed a subchain off the
/// main chain.  Eventually, you are aiming to end up with a chain whose input and output are both `()`, at which point
/// it may be mounted into the audio thread, and exchanged for a handle.
///
/// Your first call will set the chain's input and output type depending on how you begin building it.  To keep this
/// reasonably simple and prevent compile times from exploding, you will often have to specify the type of the input,
/// should you want that input to be something besides `()`.  A vast majority of the time, you will start with `()`,
/// e.g. reading from an audio file, or starting with a constant, etc.
///
/// To read types  and error messages here, the innermost type is the first signal in the chain.  Signal evaluation
/// happens bottom to top, kind of like iterators, though the mechanism is an internal implementation detail and not
/// something you have direct access to.
///
/// I/O happens "late".  Building the chain only leads to errors for validation.  Files open, threads start, etc. only
/// when the chain gets mounted.
pub struct Chain<'p, S> {
    pub(crate) inner: S,
    pub(crate) program: &'p crate::program::Program,
}

impl<'p, S> IntoSignal for Chain<'p, S>
where
    S: IntoSignal,
{
    type Signal = S::Signal;

    fn into_signal(self) -> IntoSignalResult<Self> {
        self.inner.into_signal()
    }
}

/// Marker type for an empty chain that hasn't been given a signal yet
pub struct EmptyChain;

impl<'p> Chain<'p, EmptyChain> {
    /// Create a new empty chain
    pub(crate) fn new(program: &'p crate::program::Program) -> Self {
        Chain {
            inner: EmptyChain,
            program,
        }
    }

    /// Create a chain with a signal (internal use)
    pub(crate) fn with<S: IntoSignal>(
        signal: S,
        program: &'p crate::program::Program,
    ) -> Chain<'p, S> {
        Chain {
            inner: signal,
            program,
        }
    }
    /// Start with a constant or another signal
    pub fn start_as<S: IntoSignal>(self, initial: S) -> Chain<'p, S> {
        Chain {
            inner: initial,
            program: self.program,
        }
    }

    /// Start with a constant value
    pub fn start_as_constant<T>(self, value: T) -> Chain<'p, T>
    where
        T: IntoSignal,
    {
        Chain {
            inner: value,
            program: self.program,
        }
    }

    /// Start a chain which wants an input of type `I`, which will be available as the output.
    ///
    /// This is used e.g. to set up recursion, as such chains can be tacked onto the end of other chains, should the output and input types match up.
    pub fn taking_input<I: 'static>(
        self,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = I, Output = I>>> {
        Chain {
            inner: sigs::StartFromInputSignalConfig::new(),
            program: self.program,
        }
    }

    /// Start a chain which reads from a slot.
    pub fn read_slot<T>(
        self,
        slot: &sigs::Slot<T>,
        initial_value: T,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = (), Output = T>>>
    where
        T: Clone + Send + Sync + 'static,
    {
        // Track slot usage
        self.program
            .state
            .write()
            .unwrap()
            .resources
            .lock()
            .unwrap()
            .slots
            .insert(slot.slot_id);

        Chain {
            inner: slot.read_signal(initial_value),
            program: self.program,
        }
    }

    /// Start a chain which reads from a slot, then includes whether or not the slot changed this block.
    ///
    /// Returns `(T, bool)`.
    pub fn read_slot_and_changed<T>(
        self,
        slot: &sigs::Slot<T>,
        initial_value: T,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = (), Output = (T, bool)>>>
    where
        T: Send + Sync + Clone + 'static,
    {
        // Track slot usage
        self.program
            .state
            .write()
            .unwrap()
            .resources
            .lock()
            .unwrap()
            .slots
            .insert(slot.slot_id);

        Chain {
            inner: slot.read_signal_and_change_flag(initial_value),
            program: self.program,
        }
    }
}

impl<'p, S> Chain<'p, S> {
    /// Mount this chain into the program as a fragment
    pub fn mount(self) -> crate::error::Result<()>
    where
        S: IntoSignal,
        S::Signal: Signal<Input = (), Output = ()> + 'static,
    {
        self.program.add_fragment(self)
    }
}

impl<'p, S: IntoSignal> Chain<'p, S> {
    /// Read from a delay line using this chain's output as the delay amount
    pub fn read_delay_line<T>(
        self,
        delay_line: &crate::signals::DelayLineHandle<T>,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = T>>>
    where
        S::Signal: Signal<Output = usize>,
        T: Clone + Send + 'static,
    {
        Chain {
            inner: crate::signals::DelayLineReadSignalConfig {
                line: delay_line.inner.clone(),
                parent: self.inner,
            },
            program: self.program,
        }
    }

    /// Write to a delay line using this chain's output
    pub fn write_delay_line<T>(
        self,
        delay_line: &crate::signals::DelayLineHandle<T>,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = ()>>>
    where
        S::Signal: Signal<Output = T>,
        T: Clone + Send + 'static,
    {
        self.write_delay_line_with_merger(delay_line, |old: &mut T, new: &T| *old = new.clone())
    }

    /// Write to a delay line with a custom merger function
    pub fn write_delay_line_with_merger<T, M>(
        self,
        delay_line: &crate::signals::DelayLineHandle<T>,
        merger: M,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = ()>>>
    where
        S::Signal: Signal<Output = T>,
        T: Clone + Send + 'static,
        M: FnMut(&mut T, &T) + Send + Sync + 'static,
    {
        Chain {
            inner: crate::signals::DelayLineWriteSignalConfig {
                line: delay_line.inner.clone(),
                parent: self.inner,
                merger,
            },
            program: self.program,
        }
    }
    /// Send this chain to the audio device.
    ///
    /// You must specify the format.  Synthizer cannot autodetect this because of the flexibility it allows (e.g. what
    /// is the format of multiplying two signals with different formats?).  If you specify a format which has more
    /// channels than your signal outputs, the extra channels will be zeroed.  If you specify a format with less, the
    /// extra channels are dropped.
    pub fn to_audio_device(
        self,
        format: ChannelFormat,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = ()>>>
    where
        IntoSignalOutput<S>: AudioFrame<f64> + Clone,
    {
        Chain {
            inner: sigs::AudioOutputSignalConfig::new(self.inner, format),
            program: self.program,
        }
    }

    /// Convert this chain's input type to another type, capping the chain with a signal that will use the `Default`
    /// implementation on whatever input type is currently wanted.
    ///
    /// This annoying function exists because Rust does not have specialization.  What we want to be able to do is to
    /// combine signals which don't have inputs with signals that do when performing mathematical operations.  Ideally,
    /// we would specialize the mathematical traits.  Unfortunately we cannot do that.  The primary use of this method
    /// is essentially to say "okay, I know the other side has some bigger input, but this side doesn't need any input, I
    /// promise".
    ///
    /// That is not the only use: sometimes you do legitimately want to feed a signal zeros or some other default value.
    pub fn discard_and_default<NewInputType>(
        self,
    ) -> Chain<
        'p,
        impl IntoSignal<
            Signal = impl Signal<
                Input = NewInputType,
                Output = IntoSignalOutput<S>,
                State = IntoSignalState<S>,
            >,
        >,
    >
    where
        IntoSignalInput<S>: Default,
        S::Signal: 'static,
        NewInputType: 'static,
    {
        Chain {
            inner: sigs::ConsumeInputSignalConfig::<_, NewInputType>::new(self.inner),
            program: self.program,
        }
    }

    /// Divide this chain's output by the sample rate of the library.
    ///
    /// This is mostly used to convert a frequency (HZ) to an increment per sample, e.g. when building sine waves.
    pub fn divide_by_sr(
        self,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = f64>>>
    where
        S::Signal: Signal<Output = f64>,
        IntoSignalInput<S>: Default + Clone,
        IntoSignalOutput<S>: Clone,
    {
        let newsig = sigs::MapSignalConfig::new(self.inner, |x| x / (config::SR as f64));
        Chain {
            inner: newsig,
            program: self.program,
        }
    }

    /// Convert the output of this chain into a different type.
    pub fn output_into<T>(
        self,
    ) -> Chain<
        'p,
        impl IntoSignal<
            Signal = impl Signal<Input = IntoSignalInput<S>, Output = T, State = IntoSignalState<S>>,
        >,
    >
    where
        T: From<IntoSignalOutput<S>>,
        IntoSignalOutput<S>: Clone,
        T: 'static,
    {
        Chain {
            inner: sigs::ConvertOutputConfig::<S, T>::new(self.inner),
            program: self.program,
        }
    }

    /// Push a periodic summation onto this chain.
    ///
    /// The input will be taken from whatever signal is here already, and the period is specified here as a constant.
    /// For example, if using a period of `1.0` and a signal `0.1`, you get `0.0 0.1 0.2 ... 0.9 0.0 0.0` (the final
    /// value is not included, but in practice you may get one arbitrarily close to that).
    pub fn periodic_sum(
        self,
        period: f64,
        initial_value: f64,
    ) -> Chain<'p, sigs::PeriodicF64Config<S>>
    where
        S: IntoSignal,
        S::Signal: Signal<Output = f64>,
    {
        Chain {
            inner: sigs::PeriodicF64Config {
                frequency: self.inner,
                period,
                initial_value,
            },
            program: self.program,
        }
    }

    /// Take the sine of this chain.
    pub fn sin(self) -> Chain<'p, sigs::SinSignalConfig<S>>
    where
        S: IntoSignal,
        S::Signal: Signal<Output = f64>,
    {
        Chain {
            inner: sigs::SinSignalConfig {
                wrapped: self.inner,
            },
            program: self.program,
        }
    }

    /// Inline version of `*`.
    ///
    /// This lets you continue the `.` syntax without having to use more variables.
    pub fn inline_mul<T>(self, other: Chain<'p, T>) -> <Self as std::ops::Mul<Chain<'p, T>>>::Output
    where
        Self: std::ops::Mul<Chain<'p, T>>,
        IntoSignalInput<S>: Clone,
        T: IntoSignal,
    {
        self * other
    }

    pub fn join<S2>(
        self,
        other: Chain<'p, S2>,
    ) -> Chain<
        'p,
        impl IntoSignal<
            Signal = impl Signal<
                Input = (IntoSignalInput<S>, IntoSignalInput<S2>),
                Output = (IntoSignalOutput<S>, IntoSignalOutput<S2>),
            >,
        >,
    >
    where
        S2: IntoSignal,
        IntoSignalOutput<S>: Clone,
        IntoSignalOutput<S2>: Clone,
        IntoSignalInput<S>: Clone,
        IntoSignalInput<S2>: Clone,
    {
        assert!(
            std::ptr::eq(self.program, other.program),
            "Cannot join chains from different programs"
        );
        Chain {
            inner: sigs::JoinSignalConfig::new(self.inner, other.inner),
            program: self.program,
        }
    }

    pub fn map<F, I, O>(
        self,
        func: F,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = O>>>
    where
        F: FnMut(I) -> O + Send + Sync + 'static,
        S::Signal: Signal<Output = I>,
        I: Clone,
        O: Send + Sync + 'static,
    {
        Chain {
            inner: sigs::MapSignalConfig::new(self.inner, func),
            program: self.program,
        }
    }

    pub fn map_input<F, I, IResult>(
        self,
        func: F,
    ) -> Chain<'p, impl IntoSignal<Signal = impl Signal<Input = I, Output = IntoSignalOutput<S>>>>
    where
        F: FnMut(I) -> IResult + Send + Sync + 'static,
        S::Signal: Signal<Input = IResult>,
        I: Clone + Send + 'static,
        IResult: Send + Sync + 'static,
    {
        Chain {
            inner: sigs::MapInputSignalConfig::new(self.inner, func),
            program: self.program,
        }
    }

    /// Map a closure over each entry in each audio frame.
    ///
    /// The closure gets the channel index and a reference to the value, and should return the new value.
    ///
    /// For example, this could be used to multiply each channel by a constant.
    ///
    /// `T` is the type of the data in the frame, usually inferred.
    pub fn map_frame<T, F>(
        self,
        closure: F,
    ) -> Chain<
        'p,
        impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = IntoSignalOutput<S>>>,
    >
    where
        IntoSignalOutput<S>: AudioFrame<T>,
        F: FnMut(usize, &T) -> T + Send + Sync + 'static,
        T: Copy + Default + 'static,
    {
        Chain {
            inner: sigs::MapFrameSignalConfig::new(self.inner, closure),
            program: self.program,
        }
    }
    /// Bypass another chain.
    ///
    /// This is a little bit like join.  The output is a tuple `(original, other)`.  The difference is that the input of
    /// `other` is the output of `original`.  Put more simply, the resulting chain outputs the value of this chain
    /// right now, plus the result of processing this chain's output through the other chain.
    ///
    /// As a concrete application, this can be used to get a signal and a delayed copy, if the bypassed chain is into
    /// and then from a delay line.
    pub fn bypass<C>(
        self,
        other: Chain<'p, C>,
    ) -> Chain<
        'p,
        impl IntoSignal<
            Signal = impl Signal<
                Input = IntoSignalInput<S>,
                Output = (IntoSignalOutput<S>, IntoSignalOutput<C>),
            >,
        >,
    >
    where
        C: IntoSignal,
        IntoSignalOutput<S>: Clone,
        IntoSignalOutput<C>: Clone,
        C::Signal: Signal<Input = IntoSignalOutput<S>>,
    {
        assert!(
            std::ptr::eq(self.program, other.program),
            "Cannot bypass chains from different programs"
        );
        Chain {
            inner: sigs::BypassSignalConfig::new(self.inner, other.inner),
            program: self.program,
        }
    }

    /// Connect this chain's output to another chain's input.
    ///
    /// This is like function composition: the output of this chain becomes the input to the next chain.
    /// The resulting chain has the same input as this chain and the output of the other chain.
    pub fn and_then<C>(
        self,
        other: Chain<'p, C>,
    ) -> Chain<
        'p,
        impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = IntoSignalOutput<C>>>,
    >
    where
        S: 'static,
        C: IntoSignal + 'static,
        IntoSignalOutput<S>: Clone,
        C::Signal: Signal<Input = IntoSignalOutput<S>>,
    {
        assert!(
            std::ptr::eq(self.program, other.program),
            "Cannot and_then chains from different programs"
        );
        Chain {
            inner: sigs::AndThenConfig {
                left: self.inner,
                right: other.inner,
            },
            program: self.program,
        }
    }
}

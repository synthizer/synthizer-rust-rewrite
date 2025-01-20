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
pub struct Chain<S> {
    pub(crate) inner: S,
}

impl<S> IntoSignal for Chain<S>
where
    S: IntoSignal,
{
    type Signal = S::Signal;

    fn into_signal(self) -> IntoSignalResult<Self> {
        self.inner.into_signal()
    }

    fn trace<F: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        inserter: &mut F,
    ) -> crate::Result<()> {
        self.inner.trace(inserter)?;
        Ok(())
    }
}

#[doc(hidden)]
pub struct ChainConstructors;

impl Chain<ChainConstructors> {
    /// Start a chain.
    ///
    /// `initial` can be one of a few things.  The two most common are another chain or a constant.
    pub fn new<S: IntoSignal>(initial: S) -> Chain<S> {
        Chain { inner: initial }
    }

    /// Start a chain which wants an input of type `I`, which will be available as the output.
    ///
    /// This is used e.g. to set up recursion, as such chains can be tacked onto the end of other chains, should the output and input types match up.
    pub fn taking_input<I: 'static>(
    ) -> Chain<impl IntoSignal<Signal = impl Signal<Input = I, Output = I>>> {
        Chain {
            inner: sigs::StartFromInputSignalConfig::new(),
        }
    }

    /// Start a chain which reads from a slot.
    pub fn read_slot<T>(
        slot: &sigs::Slot<T>,
        initial_value: T,
    ) -> Chain<impl IntoSignal<Signal = impl Signal<Input = (), Output = T>>>
    where
        T: Clone + Send + Sync + 'static,
    {
        Chain {
            inner: slot.read_signal(initial_value),
        }
    }

    /// Start a chain which reads from a slot, then includes whether or not the slot changed this block.
    ///
    /// Returns `(T, bool)`.
    pub fn read_slot_and_changed<T>(
        slot: &sigs::Slot<T>,
        initial_value: T,
    ) -> Chain<impl IntoSignal<Signal = impl Signal<Input = (), Output = (T, bool)>>>
    where
        T: Send + Sync + Clone + 'static,
    {
        Chain {
            inner: slot.read_signal_and_change_flag(initial_value),
        }
    }
}

impl<S: IntoSignal> Chain<S> {
    /// Send this chain to the audio device.
    ///
    /// You must specify the format.  Synthizer cannot autodetect this because of the flexibility it allows (e.g. what
    /// is the format of multiplying two signals with different formats?).  If you specify a format which has more
    /// channels than your signal outputs, the extra channels will be zeroed.  If you specify a format with less, the
    /// extra channels are dropped.
    pub fn to_audio_device(
        self,
        format: ChannelFormat,
    ) -> Chain<impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = ()>>>
    where
        IntoSignalOutput<S>: AudioFrame<f64> + Clone,
    {
        Chain {
            inner: sigs::AudioOutputSignalConfig::new(self.inner, format),
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
        }
    }

    /// Divide this chain's output by the sample rate of the library.
    ///
    /// This is mostly used to convert a frequency (HZ) to an increment per sample, e.g. when building sine waves.
    pub fn divide_by_sr(
        self,
    ) -> Chain<impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = f64>>>
    where
        S::Signal: Signal<Output = f64>,
        IntoSignalInput<S>: Default + Clone,
        IntoSignalOutput<S>: Clone,
    {
        let newsig = sigs::MapSignalConfig::new(self.inner, |x| x / (config::SR as f64));
        Chain { inner: newsig }
    }

    /// Convert the output of this chain into a different type.
    pub fn output_into<T>(
        self,
    ) -> Chain<
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
        }
    }

    /// Push a periodic summation onto this chain.
    ///
    /// The input will be taken from whatever signal is here already, and the period is specified herer as a constant.
    /// For example, if using a period of `1.0` and a signal `0.1`, you get `0.0 0.1 0.2 ... 0.9 0.0 0.0` (the final
    /// value is not included, but in practice you may get one arbitrarily close to that).
    pub fn periodic_sum(self, period: f64, initial_value: f64) -> Chain<sigs::PeriodicF64Config<S>>
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
        }
    }

    /// Take the sine of this chain.
    pub fn sin(self) -> Chain<sigs::SinSignalConfig<S>>
    where
        S: IntoSignal,
        S::Signal: Signal<Output = f64>,
    {
        Chain {
            inner: sigs::SinSignalConfig {
                wrapped: self.inner,
            },
        }
    }

    /// Inline version of `*`.
    ///
    /// This lets you continue the `.` syntax without having to use more variables.
    pub fn inline_mul<T>(self, other: Chain<T>) -> <Self as std::ops::Mul<Chain<T>>>::Output
    where
        Self: std::ops::Mul<Chain<T>>,
        IntoSignalInput<S>: Clone,
        T: IntoSignal,
    {
        self * other
    }

    /// Box this signal.
    ///
    /// This simplifies the type, at a performance cost.  If you do not put this boxed signal in a recursive path, the
    /// performance cost is minimal.
    pub fn boxed<I, O>(self) -> Chain<sigs::BoxedSignalConfig<I, O>>
    where
        I: Copy + Send + Sync + 'static,
        O: Copy + Send + Sync + 'static,
        S: Send + Sync + 'static,
        S::Signal: Signal<Input = I, Output = O>,
    {
        Chain {
            inner: sigs::BoxedSignalConfig::new(self.inner),
        }
    }

    pub fn join<S2>(
        self,
        other: Chain<S2>,
    ) -> Chain<
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
        Chain {
            inner: sigs::JoinSignalConfig::new(self.inner, other.inner),
        }
    }

    pub fn map<F, I, O>(
        self,
        func: F,
    ) -> Chain<impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = O>>>
    where
        F: FnMut(I) -> O + Send + Sync + 'static,
        S::Signal: Signal<Output = I>,
        I: Clone,
        O: Send + Sync + 'static,
    {
        Chain {
            inner: sigs::MapSignalConfig::new(self.inner, func),
        }
    }

    pub fn map_input<F, I, IResult>(
        self,
        func: F,
    ) -> Chain<impl IntoSignal<Signal = impl Signal<Input = I, Output = IntoSignalOutput<S>>>>
    where
        F: FnMut(I) -> IResult + Send + Sync + 'static,
        S::Signal: Signal<Input = IResult>,
        I: Clone + Send + 'static,
        IResult: Send + Sync + 'static,
    {
        Chain {
            inner: sigs::MapInputSignalConfig::new(self.inner, func),
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
        impl IntoSignal<Signal = impl Signal<Input = IntoSignalInput<S>, Output = IntoSignalOutput<S>>>,
    >
    where
        IntoSignalOutput<S>: AudioFrame<T>,
        F: FnMut(usize, &T) -> T + Send + Sync + 'static,
        T: Copy + Default + 'static,
    {
        Chain {
            inner: sigs::MapFrameSignalConfig::new(self.inner, closure),
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
        other: Chain<C>,
    ) -> Chain<
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
        Chain {
            inner: sigs::BypassSignalConfig::new(self.inner, other.inner),
        }
    }
}

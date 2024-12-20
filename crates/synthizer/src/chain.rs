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
}

/// Start a chain which reads from a slot.
pub fn read_slot<T>(
    slot: &sigs::Slot<T>,
    initial_value: T,
) -> Chain<impl IntoSignal<Signal = impl for<'a> Signal<Input<'a> = (), Output<'a> = T>>>
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
) -> Chain<impl IntoSignal<Signal = impl for<'a> Signal<Input<'a> = (), Output<'a> = (T, bool)>>>
where
    T: Send + Sync + Clone + 'static,
{
    Chain {
        inner: slot.read_signal_and_change_flag(initial_value),
    }
}

impl<S: IntoSignal> Chain<S> {
    /// Start a chain.
    ///
    /// `initial` can be one of a few things.  The two most common are another chain or a constant.
    pub fn new(initial: S) -> Chain<S> {
        Chain { inner: initial }
    }

    /// Send this chain to the audio device.
    pub fn to_audio_device(
        self,
    ) -> Chain<
        impl IntoSignal<
            Signal = impl for<'a> Signal<Input<'a> = IntoSignalInput<'a, S>, Output<'a> = ()>,
        >,
    >
    where
        S::Signal: for<'a> Signal<Output<'a> = f64>,
    {
        Chain {
            inner: sigs::AudioOutputSignalConfig::new(self.inner),
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
            Signal = impl for<'a> Signal<
                Input<'a> = NewInputType,
                Output<'a> = IntoSignalOutput<'a, S>,
                State = IntoSignalState<S>,
                Parameters = IntoSignalParameters<S>,
            >,
        >,
    >
    where
        for<'a> IntoSignalInput<'a, S>: Default,
        S::Signal: 'static,
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
    ) -> Chain<
        impl IntoSignal<
            Signal = impl for<'a> Signal<Input<'a> = IntoSignalInput<'a, S>, Output<'a> = f64>,
        >,
    >
    where
        for<'a> S::Signal: Signal<Output<'a> = f64>,
        for<'a> IntoSignalInput<'a, S>: Default + Clone,
        for<'a> IntoSignalOutput<'a, S>: Clone,
    {
        let converted = self.output_into::<f64>();
        let sr = Chain::new(config::SR as f64).discard_and_default::<IntoSignalInput<S>>();
        let done = converted / sr;
        Chain { inner: done.inner }
    }

    /// Convert the output of this chain into a different type.
    pub fn output_into<T>(
        self,
    ) -> Chain<
        impl IntoSignal<
            Signal = impl for<'a> Signal<
                Input<'a> = IntoSignalInput<'a, S>,
                Output<'a> = T,
                State = IntoSignalState<S>,
                Parameters = IntoSignalParameters<S>,
            >,
        >,
    >
    where
        for<'a> T: From<IntoSignalOutput<'a, S>>,
        for<'a> IntoSignalOutput<'a, S>: Clone,
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
        for<'a> S::Signal: Signal<Output<'a> = f64>,
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
        for<'a> S::Signal: Signal<Output<'a> = f64>,
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
    pub fn inline_mul<T>(self, other: T) -> Chain<<Self as std::ops::Mul<T>>::Output>
    where
        Self: std::ops::Mul<T>,
    {
        Chain {
            inner: self * other,
        }
    }
}

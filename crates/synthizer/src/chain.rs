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

    fn into_signal(self) -> crate::Result<Self::Signal> {
        self.inner.into_signal()
    }
}

impl<S: IntoSignal> Chain<S> {
    /// Start a chain.
    ///
    /// `initial` can be one of a few things.  The two most common are another chain or a constant.
    pub fn new(initial: S) -> Self {
        Self { inner: initial }
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
}

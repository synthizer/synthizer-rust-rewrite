//! Signals which know how to call into() on various parts of their parent signals.
//!
//! You access this with `.cast_xxx()`.
use std::marker::PhantomData as PD;

use crate::core_traits::*;

/// Converts the output of the upstream signal into the input of the downstream signal, if `O::Output: Into<DType>`.
pub struct ConvertOutput<OSig, DType>(PD<*mut OSig>, PD<*mut DType>);

impl<Sig, DType> ConvertOutput<Sig, DType> {
    pub(crate) fn new() -> Self {
        Self(PD, PD)
    }
}

impl<Sig, DType> Signal for ConvertOutput<Sig, DType>
where
    Sig: Signal,
    Sig::Output: Into<DType>,
{
    type Output = DType;
    type Input = Sig::Input;
    type Parameters = Sig::Parameters;
    type State = Sig::State;

    fn tick1<D: SignalDestination<Self::Output>>(
        ctx: &mut SignalExecutionContext<'_, Self::State, Self::Parameters>,
        input: &'_ Self::Input,
        mut destination: D,
    ) {
        Sig::tick1(ctx, input, |x: Sig::Output| {
            let y: DType = x.into();
            destination.send(y)
        })
    }
}

impl<Sig, DType> IntoSignal for ConvertOutput<Sig, DType>
where
    Sig: Signal,
    Sig::Output: Into<DType>,
{
    type Signal = Self;

    fn into_signal(self) -> crate::Result<Self::Signal> {
        Ok(ConvertOutput::new())
    }
}

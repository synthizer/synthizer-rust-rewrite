//! Signals which know how to call into() on various parts of their parent signals.
//!
//! You access this with `.cast_xxx()`.
use std::marker::PhantomData as PD;

use crate::context::*;
use crate::core_traits::*;
use crate::error::Result;

/// Converts the output of the upstream signal into the input of the downstream signal, if `O::Output: Into<DType>`.
pub struct ConvertOutput<OSig, DType>(OSig, PD<DType>);
pub struct ConvertOutputConfig<OSig, DType>(OSig, PD<DType>);

impl<Sig, DType> ConvertOutput<Sig, DType> {
    pub(crate) fn new(sig: Sig) -> Self {
        Self(sig, PD)
    }
}

impl<Sig, DType> ConvertOutputConfig<Sig, DType> {
    pub(crate) fn new(sig: Sig) -> Self {
        Self(sig, PD)
    }
}

unsafe impl<Sig, DType> Signal for ConvertOutput<Sig, DType>
where
    Sig: Signal,
    Sig::Output: Into<DType>,
    DType: 'static,
{
    type Output = DType;
    type Input = Sig::Input;
    type State = Sig::State;

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        Sig::on_block_start(ctx, state);
    }

    fn tick_frame(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let output = Sig::tick_frame(ctx, input, state);
        output.into()
    }
}

// DType is not part of the signal. It is only a record of the signal's output type. Consequently these are actually
// correct.
unsafe impl<Sig: Send, DType> Send for ConvertOutput<Sig, DType> {}
unsafe impl<Sig: Sync, DType> Sync for ConvertOutput<Sig, DType> {}

impl<Sig, DType> IntoSignal for ConvertOutputConfig<Sig, DType>
where
    Sig: IntoSignal,
    IntoSignalOutput<Sig>: Into<DType>,
    DType: 'static,
{
    type Signal = ConvertOutput<Sig::Signal, DType>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let inner = self.0.into_signal()?;
        Ok(ReadySignal {
            signal: ConvertOutput::new(inner.signal),
            state: inner.state,
        })
    }

    fn trace<F: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        inserter: &mut F,
    ) -> Result<()> {
        self.0.trace(inserter)?;
        Ok(())
    }
}

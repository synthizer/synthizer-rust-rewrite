//! Signals which know how to call into() on various parts of their parent signals.
//!
//! You access this with `.cast_xxx()`.
use std::marker::PhantomData as PD;

use crate::context::*;
use crate::core_traits::*;

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
    for<'a> Sig::Output<'a>: Into<DType> + Clone,
    DType: 'static,
{
    type Output<'ol> = DType;
    type Input<'il> = Sig::Input<'il>;
    type Parameters = Sig::Parameters;
    type State = Sig::State;

    fn on_block_start(
        ctx: &SignalExecutionContext<'_, '_>,
        params: &Self::Parameters,
        state: &mut Self::State,
    ) {
        Sig::on_block_start(ctx, params, state);
    }

    fn tick<'il, 'ol, D, const N: usize>(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: [Self::Input<'il>; N],
        params: &Self::Parameters,
        state: &mut Self::State,
        destination: D,
    ) where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        D: SignalDestination<Self::Output<'ol>, N>,
    {
        Sig::tick::<_, N>(ctx, input, params, state, |x: [Sig::Output<'ol>; N]| {
            destination.send(x.map(Into::into));
        });
    }

    fn trace_slots<
        F: FnMut(
            crate::unique_id::UniqueId,
            std::sync::Arc<dyn std::any::Any + Send + Sync + 'static>,
        ),
    >(
        state: &Self::State,
        parameters: &Self::Parameters,
        inserter: &mut F,
    ) {
        Sig::trace_slots(state, parameters, inserter);
    }
}

// DType is not part of the signal. It is only a record of the signal's output type. Consequently these are actually
// correct.
unsafe impl<Sig: Send, DType> Send for ConvertOutput<Sig, DType> {}
unsafe impl<Sig: Sync, DType> Sync for ConvertOutput<Sig, DType> {}

impl<Sig, DType> IntoSignal for ConvertOutputConfig<Sig, DType>
where
    Sig: IntoSignal,
    for<'a> IntoSignalOutput<'a, Sig>: Into<DType> + Clone,
    DType: 'static,
{
    type Signal = ConvertOutput<Sig::Signal, DType>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let inner = self.0.into_signal()?;
        Ok(ReadySignal {
            signal: ConvertOutput::new(inner.signal),
            state: inner.state,
            parameters: inner.parameters,
        })
    }
}

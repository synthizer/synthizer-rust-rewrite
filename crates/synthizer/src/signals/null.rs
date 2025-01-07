use crate::context::*;
use crate::core_traits::*;
use crate::error::*;

/// The null signal: a signal which takes nothing and outputs nothing.
///
/// We have to start chains somehow.  This is the "empty chain" signal: it can be mounted but if it is, it does nothing
/// at all.
pub struct NullSignal(());

unsafe impl Signal for NullSignal {
    type Input<'il> = ();
    type Output<'ol> = ();
    type State = ();

    fn tick<'il, 'ol, 's, I, const N: usize>(
        _ctx: &'_ SignalExecutionContext<'_, '_>,
        _input: I,
        _state: &'s mut Self::State,
    ) -> impl ValueProvider<Self::Output<'ol>>
    where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        's: 'ol,
        I: ValueProvider<Self::Input<'il>> + Sized,
    {
        FixedValueProvider::<_, N>::new(())
    }

    fn on_block_start(_ctx: &SignalExecutionContext<'_, '_>, _state: &mut Self::State) {}
}

impl IntoSignal for NullSignal {
    type Signal = NullSignal;

    fn into_signal(self) -> IntoSignalResult<Self> {
        Ok(ReadySignal {
            signal: NullSignal::new(),
            state: (),
        })
    }
    fn trace<F: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        _inserter: &mut F,
    ) -> Result<()> {
        Ok(())
    }
}

impl NullSignal {
    pub(crate) fn new() -> NullSignal {
        NullSignal(())
    }
}

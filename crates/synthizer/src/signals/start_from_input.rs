use std::marker::PhantomData as PD;

use crate::core_traits::*;

/// A signal which converts its input to its output.
///
/// This goes at the beginning of chains which want to start with something besides `()`: the input flows "upstream"
/// until it hits this signal, then turns around and flows "downstream".
pub struct StartFromInputSignalConfig<T>(PD<T>);
pub struct StartFromInputSignal<T>(PD<T>);

unsafe impl<T> Send for StartFromInputSignal<T> {}
unsafe impl<T> Sync for StartFromInputSignal<T> {}

unsafe impl<T> Signal for StartFromInputSignal<T>
where
    T: 'static,
{
    type Input<'il> = T;
    type Output<'ol> = T;
    type State = ();

    fn on_block_start(
        _ctx: &crate::context::SignalExecutionContext<'_, '_>,
        _state: &mut Self::State,
    ) {
        // Nothing to do.
    }

    fn tick<'il, 'ol, 's, I, const N: usize>(
        _ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: I,
        _state: &'s mut Self::State,
    ) -> impl ValueProvider<Self::Output<'ol>>
    where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        's: 'ol,
        I: ValueProvider<Self::Input<'il>> + Sized,
    {
        input
    }
}

impl<T> IntoSignal for StartFromInputSignalConfig<T>
where
    StartFromInputSignal<T>: Signal<State = ()>,
{
    type Signal = StartFromInputSignal<T>;

    fn into_signal(self) -> crate::Result<ReadySignal<Self::Signal, IntoSignalState<Self>>> {
        Ok(ReadySignal {
            signal: StartFromInputSignal(PD),
            state: (),
        })
    }

    fn trace<F: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        _inserter: &mut F,
    ) -> crate::Result<()> {
        // Nothing.
        Ok(())
    }
}

impl<T> StartFromInputSignalConfig<T> {
    pub(crate) fn new() -> Self {
        Self(PD)
    }
}

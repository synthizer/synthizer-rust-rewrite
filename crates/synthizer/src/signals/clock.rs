//! Read the current sample.
use crate::core_traits::*;

pub struct Clock(());

unsafe impl Signal for Clock {
    type Input = ();
    type Output = u64;
    type State = ();
    type Parameters = ();

    fn tick1<D: SignalDestination<Self::Output>>(
        ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        _input: &'_ Self::Input,
        destination: D,
    ) {
        destination.send(ctx.fixed.time);
    }
}

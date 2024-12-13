//! Read the current sample.
use crate::core_traits::*;

pub struct Clock(());

impl Signal for Clock {
    type Input = ();
    type Output = u64;
    type State = ();
    type Parameters = ();

    fn tick1<D: SignalDestination<Self::Output>>(
        ctx: &mut SignalExecutionContext<'_, Self::State, Self::Parameters>,
        _input: &'_ Self::Input,
        mut destination: D,
    ) {
        destination.send(ctx.time);
    }
}

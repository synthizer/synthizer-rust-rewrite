use crate::context::*;
use crate::core_traits::*;
use crate::error::Result;

/// Takes the signal on the left, and feeds its output to the signal on the right.  The signal on the left will be
/// evaluated first.
///
/// This allows "filling holes".  For example, one might map a set of signals into a struct for later use, then use
/// `and_then` to pass it to a signal expecting that struct.  This is what allows chains to embed other chains in them,
/// and to have recursion.  In other words, higher level helpers use this as a building block.
pub struct AndThen<S1, S2>(S1, S2);

unsafe impl<S1, S2> Signal for AndThen<S1, S2>
where
    S1: Signal,
    S2: Signal<Input = S1::Output>,
    S1: 'static,
    S2: 'static,
{
    type Input = S1::Input;
    type Output = S2::Output;
    type State = (S1::State, S2::State);

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        S1::on_block_start(ctx, &mut state.0);
        S2::on_block_start(ctx, &mut state.1);
    }

    fn tick_frame(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let left_output = S1::tick_frame(ctx, input, &mut state.0);
        S2::tick_frame(ctx, left_output, &mut state.1)
    }
}

pub struct AndThenConfig<S1, S2> {
    left: S1,
    right: S2,
}

impl<S1, S2> IntoSignal for AndThenConfig<S1, S2>
where
    S1: IntoSignal + 'static,
    S2: IntoSignal + 'static,
    S1::Signal: Signal<Output = <S2::Signal as Signal>::Input>,
    S1::Signal: 'static,
    S2::Signal: 'static,
{
    type Signal = AndThen<S1::Signal, S2::Signal>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let s1 = self.left.into_signal()?;
        let s2 = self.right.into_signal()?;
        Ok(ReadySignal {
            signal: AndThen::new(s1.signal, s2.signal),
            state: (s1.state, s2.state),
        })
    }

    fn trace<F: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        inserter: &mut F,
    ) -> Result<()> {
        self.left.trace(inserter)?;
        self.right.trace(inserter)?;
        Ok(())
    }
}

impl<S1, S2> AndThen<S1, S2> {
    pub(crate) fn new(s1: S1, s2: S2) -> Self {
        AndThen(s1, s2)
    }
}

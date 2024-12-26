use crate::context::*;
use crate::core_traits::*;

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
    for<'a> S2: Signal<Input<'a> = S1::Output<'a>>,
    S1: 'static,
    S2: 'static,
{
    type Input<'il> = S1::Input<'il>;
    type Output<'ol> = S2::Output<'ol>;
    type State = (S1::State, S2::State);

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        S1::on_block_start(ctx, &mut state.0);
        S2::on_block_start(ctx, &mut state.1);
    }

    fn tick<'il, 'ol, I, const N: usize>(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: I,
        state: &mut Self::State,
    ) -> impl ValueProvider<Self::Output<'ol>>
    where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        I: ValueProvider<Self::Input<'il>> + Sized,
    {
        let left = S1::tick::<_, N>(ctx, input, &mut state.0);
        S2::tick::<_, N>(ctx, left, &mut state.1)
    }

    fn trace_slots<
        F: FnMut(
            crate::unique_id::UniqueId,
            std::sync::Arc<dyn std::any::Any + Send + Sync + 'static>,
        ),
    >(
        state: &Self::State,
        inserter: &mut F,
    ) {
        S1::trace_slots(&state.0, inserter);
        S2::trace_slots(&state.1, inserter);
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
    for<'a> S1::Signal: Signal<Output<'a> = <S2::Signal as Signal>::Input<'a>>,
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
}

impl<S1, S2> AndThen<S1, S2> {
    pub(crate) fn new(s1: S1, s2: S2) -> Self {
        AndThen(s1, s2)
    }
}

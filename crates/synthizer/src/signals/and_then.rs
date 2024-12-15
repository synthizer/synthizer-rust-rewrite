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
    S2: Signal<Input = S1::Output>,
{
    type Input = S1::Input;
    type Output = S2::Output;
    type State = (S1::State, S2::State);
    type Parameters = (S1::Parameters, S2::Parameters);

    fn tick1<D: SignalDestination<Self::Output>>(
        ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        input: &'_ Self::Input,
        destination: D,
    ) {
        // LLVM should be able to see through this.  Not that that's guaranteed, but it avoids wild unsafety.
        let mut left: Option<S1::Output> = None;

        S1::tick1(&mut ctx.wrap(|s| &mut s.0, |p| &p.0), input, |x| {
            left = Some(x)
        });
        let left = left.unwrap();

        S2::tick1(&mut ctx.wrap(|s| &mut s.1, |p| &p.1), &left, destination);
    }
}

pub struct AndThenConfig<S1, S2> {
    left: S1,
    right: S2,
}

impl<S1, S2> IntoSignal for AndThenConfig<S1, S2>
where
    S1: IntoSignal,
    S2: IntoSignal,
    S1::Signal: Signal<Output = <S2::Signal as Signal>::Input>,
{
    type Signal = AndThen<S1::Signal, S2::Signal>;

    fn into_signal(self) -> crate::Result<Self::Signal> {
        let s1 = self.left.into_signal()?;
        let s2 = self.right.into_signal()?;
        Ok(AndThen::new(s1, s2))
    }
}

impl<S1, S2> AndThen<S1, S2> {
    pub(crate) fn new(s1: S1, s2: S2) -> Self {
        AndThen(s1, s2)
    }
}

//! Implements the mathematical operations between `IntoSignal`s.
use std::any::Any;

use std::ops::*;
use std::sync::Arc;

use crate::chain::Chain;
use crate::context::SignalExecutionContext;
use crate::core_traits::*;
use crate::unique_id::UniqueId;

macro_rules! impl_mathop {
    ($trait: ident, $signal_name: ident, $signal_config:ident, $method: ident) => {
        pub struct $signal_name<L, R>(L, R);
        pub struct $signal_config<L, R>(L, R);

        impl<A, B> $trait<Chain<B>> for Chain<A>
        where
            A: IntoSignal,
            B: IntoSignal,
            $signal_config<A, B>: IntoSignal,
        {
            type Output = Chain<$signal_config<A, B>>;

            fn $method(self, rhs: Chain<B>) -> Self::Output {
                Chain {
                    inner: $signal_config(self.inner, rhs.inner),
                }
            }
        }

        unsafe impl<I1, I2, O1, O2, S1, S2> Signal for $signal_name<S1, S2>
        where
            for<'il, 'ol> S1: Signal<Input<'il> = I1, Output<'ol> = O1>,
            for<'il, 'ol> S2: Signal<Input<'il> = I2, Output<'ol> = O2>,
            O1: $trait<O2>,
            O1: Clone,
            O2: Clone,
            I1: Clone + 'static,
            I2: From<I1> + Clone + 'static,
        {
            type Input<'il> = SignalInput<'il, S1>;
            type Output<'ol> = <SignalOutput<'ol, S1> as $trait<SignalOutput<'ol, S2>>>::Output;

            type State = (SignalState<S1>, SignalState<S2>);

            fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
                S1::on_block_start(ctx, &mut state.0);
                S2::on_block_start(&ctx, &mut state.1);
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
                let gathered_input = crate::array_utils::collect_iter::<_, N>(input.iter_cloned());
                let left = S1::tick::<_, N>(
                    ctx,
                    ArrayProvider::new(gathered_input.clone()),
                    &mut state.0,
                );
                let right = S2::tick::<_, N>(
                    ctx,
                    ArrayProvider::new(gathered_input.map(Into::into)),
                    &mut state.1,
                );

                // For now we will collect to an array. We may be able to do lazy computation later, but the bounds on
                // this are a mess.
                let left_iter = left.iter_cloned();
                let right_iter = right.iter_cloned();
                let arr = crate::array_utils::collect_iter::<_, N>(
                    left_iter.zip(right_iter).map(|(l, r)| l.$method(r)),
                );
                ArrayProvider::new(arr)
            }

            fn trace_slots<F: FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>)>(
                state: &Self::State,
                inserter: &mut F,
            ) {
                S1::trace_slots(&state.0, inserter);
                S2::trace_slots(&state.1, inserter);
            }
        }

        impl<I1, I2, S1, S2> IntoSignal for $signal_config<S1, S2>
        where
            S1: IntoSignal + Send + Sync,
            S2: IntoSignal + Send + Sync,
            for<'il, 'ol> S1::Signal: Signal<Input<'il> = I1, Output<'ol> = f64>,
            for<'il, 'ol> S2::Signal: Signal<Input<'il> = I2, Output<'ol> = f64>,
            I1: Clone + 'static,
            I2: From<I1> + Clone + 'static,
        {
            type Signal = $signal_name<S1::Signal, S2::Signal>;

            fn into_signal(self) -> IntoSignalResult<Self> {
                let l = self.0.into_signal()?;
                let r = self.1.into_signal()?;
                Ok(ReadySignal {
                    signal: $signal_name(l.signal, r.signal),
                    state: (l.state, r.state),
                })
            }
        }
    };
}

impl_mathop!(Add, AddSig, AddSigConfig, add);
impl_mathop!(Sub, SubSig, SubSigConfig, sub);
impl_mathop!(Mul, MulSig, MulSigConfig, mul);
impl_mathop!(Div, DivSig, DivSigConfig, div);

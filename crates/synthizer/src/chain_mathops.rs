//! Implements the mathematical operations between `IntoSignal`s.
use std::ops::*;

use crate::chain::Chain;
use crate::context::SignalExecutionContext;
use crate::core_traits::*;

macro_rules! impl_mathop {
    ($trait: ident, $signal_name: ident, $signal_config:ident, $method: ident) => {
        /// Implementation detail for chain math operations - not part of the public API
        #[doc(hidden)]
        pub struct $signal_name<L, R>(L, R);
        /// Implementation detail for chain math operations - not part of the public API
        #[doc(hidden)]
        pub struct $signal_config<L, R>(L, R);

        impl<'p, A, B> $trait<Chain<'p, B>> for Chain<'p, A>
        where
            A: IntoSignal,
            B: IntoSignal,
            $signal_config<A, B>: IntoSignal,
        {
            type Output = Chain<'p, $signal_config<A, B>>;

            fn $method(self, rhs: Chain<'p, B>) -> Self::Output {
                assert!(
                    std::ptr::eq(self.program, rhs.program),
                    concat!(
                        "Cannot ",
                        stringify!($method),
                        " chains from different programs"
                    )
                );
                Chain {
                    inner: $signal_config(self.inner, rhs.inner),
                    program: self.program,
                }
            }
        }

        unsafe impl<I1, I2, O1, O2, S1, S2> Signal for $signal_name<S1, S2>
        where
            S1: Signal<Input = I1, Output = O1>,
            S2: Signal<Input = I2, Output = O2>,
            O1: $trait<O2>,
            I1: Clone + 'static,
            I2: From<I1> + 'static,
        {
            type Input = SignalInput<S1>;
            type Output = <SignalOutput<S1> as $trait<SignalOutput<S2>>>::Output;

            type State = (SignalState<S1>, SignalState<S2>);

            fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
                S1::on_block_start(ctx, &mut state.0);
                S2::on_block_start(&ctx, &mut state.1);
            }

            fn tick_frame(
                ctx: &'_ SignalExecutionContext<'_, '_>,
                input: Self::Input,
                state: &mut Self::State,
            ) -> Self::Output {
                let input_for_right: I2 = input.clone().into();
                let left_output = S1::tick_frame(ctx, input, &mut state.0);
                let right_output = S2::tick_frame(ctx, input_for_right, &mut state.1);
                left_output.$method(right_output)
            }
        }

        impl<I1, I2, S1, S2> IntoSignal for $signal_config<S1, S2>
        where
            S1: IntoSignal + Send + Sync,
            S2: IntoSignal + Send + Sync,
            S1::Signal: Signal<Input = I1, Output = f64>,
            S2::Signal: Signal<Input = I2, Output = f64>,
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

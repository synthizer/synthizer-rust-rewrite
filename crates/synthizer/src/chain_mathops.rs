//! Implements the mathematical operations between `IntoSignal`s.
use std::ops::*;

use crate::chain::Chain;
use crate::context::SignalExecutionContext;
use crate::core_traits::*;

macro_rules! impl_mathop {
    ($trait: ident, $signal_name: ident, $signal_config:ident, $method: ident) => {
        pub struct $signal_name<L, R>(L, R);
        pub struct $signal_config<L, R>(L, R);

        impl<A, B> $trait<Chain<B>> for Chain<A>
        where
            A: IntoSignal,
            B: IntoSignal,
            A::Signal: Signal<Output = IntoSignalOutput<B>>,
            IntoSignalOutput<A>: $trait<IntoSignalOutput<B>>,
        {
            type Output = Chain<$signal_config<A, B>>;

            fn $method(self, rhs: Chain<B>) -> Self::Output {
                Chain {
                    inner: $signal_config(self.inner, rhs.inner),
                }
            }
        }

        unsafe impl<S1, S2> Signal for $signal_name<S1, S2>
        where
            S1: Signal,
            S2: Signal<Input = SignalSealedInput<S1>>,
            SignalSealedOutput<S1>: $trait<SignalSealedOutput<S2>>,
        {
            type Input = SignalSealedInput<S1>;
            type Output = <SignalSealedOutput<S1> as $trait<SignalSealedOutput<S2>>>::Output;
            type Parameters = (SignalSealedParameters<S1>, SignalSealedParameters<S2>);
            type State = (SignalSealedState<S1>, SignalSealedState<S2>);

            fn tick1<D: SignalDestination<Self::Output>>(
                ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
                input: &'_ Self::Input,
                destination: D,
            ) {
                // The complexity here is that we cannot project the context twice.  We need the left value first.
                let mut left = None;
                S1::tick1(&mut ctx.wrap(|s| &mut s.0, |p| &p.0), input, |y| {
                    left = Some(y)
                });
                S2::tick1(&mut ctx.wrap(|s| &mut s.1, |p| &p.1), input, |y| {
                    destination.send(left.unwrap().$method(y));
                })
            }
        }

        impl<S1, S2> IntoSignal for $signal_config<S1, S2>
        where
            S1: IntoSignal,
            S2: IntoSignal,
            $signal_name<S1::Signal, S2::Signal>: Signal,
        {
            type Signal = $signal_name<S1::Signal, S2::Signal>;

            fn into_signal(self) -> crate::Result<Self::Signal> {
                let l = self.0.into_signal()?;
                let r = self.1.into_signal()?;
                Ok($signal_name(l, r))
            }
        }
    };
}

impl_mathop!(Add, AddSig, AddSigConfig, add);
impl_mathop!(Sub, SubSig, SubSigConfig, sub);
impl_mathop!(Mul, MulSig, MulSigConfig, mul);
impl_mathop!(Div, DivSig, DivSigConfig, div);
impl_mathop!(BitAnd, BitAndSig, BitAndSigConfig, bitand);
impl_mathop!(BitOr, BitOrSig, BitOrSigConfig, bitor);
impl_mathop!(BitXor, BitXorSig, BitXorSigConfig, bitxor);

impl_mathop!(Rem, RemSig, RemSigConfig, rem);
impl_mathop!(Shl, ShlSig, ShlSigConfig, shl);
impl_mathop!(Shr, ShrSig, ShrSigConfig, shr);

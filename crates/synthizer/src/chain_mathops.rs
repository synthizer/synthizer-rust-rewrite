//! Implements the mathematical operations between `IntoSignal`s.
use std::any::Any;
use std::mem::MaybeUninit;
use std::ops::*;
use std::sync::Arc;

use crate::chain::Chain;
use crate::config;
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
            S2: Signal<Input = SignalInput<S1>>,
            SignalOutput<S1>: $trait<SignalOutput<S2>>,
        {
            type Input = SignalInput<S1>;
            type Output = <SignalOutput<S1> as $trait<SignalOutput<S2>>>::Output;
            type Parameters = (SignalParameters<S1>, SignalParameters<S2>);
            type State = (SignalState<S1>, SignalState<S2>);

            fn on_block_start(
                ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
            ) {
                S1::on_block_start(&mut ctx.wrap(|s| &mut s.0, |p| &p.0));
                S2::on_block_start(&mut ctx.wrap(|s| &mut s.1, |p| &p.1));
            }

            fn tick_block<
                'a,
                I: FnMut(usize) -> &'a Self::Input,
                D: ReusableSignalDestination<Self::Output>,
            >(
                ctx: &'_ mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
                mut input: I,
                mut destination: D,
            ) where
                Self::Input: 'a,
            {
                // When we perform the binary operation, left and right fold into each other and the drop is handled
                // because either the operation dropped the values itself or the final value holds them.  Dropping these
                // would thus be a double drop.
                let mut left: [MaybeUninit<SignalOutput<S1>>; config::BLOCK_SIZE] =
                    [const { MaybeUninit::uninit() }; config::BLOCK_SIZE];
                let mut right: [MaybeUninit<SignalOutput<S2>>; config::BLOCK_SIZE] =
                    [const { MaybeUninit::uninit() }; config::BLOCK_SIZE];
                let mut i = 0usize;

                S1::tick_block(&mut ctx.wrap(|s| &mut s.0, |p| &p.0), &mut input, |val| {
                    left[i].write(val);
                    i += 1;
                });

                i = 0;

                S2::tick_block(&mut ctx.wrap(|s| &mut s.1, |p| &p.1), &mut input, |val| {
                    right[i].write(val);
                    i += 1;
                });

                left.into_iter().zip(right.into_iter()).for_each(|(l, r)| {
                    let l = unsafe { l.assume_init() };
                    let r = unsafe { r.assume_init() };
                    destination.send_reusable(l.$method(r));
                });
            }

            fn trace_slots<F: FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>)>(
                state: &Self::State,
                parameters: &Self::Parameters,
                inserter: &mut F,
            ) {
                S1::trace_slots(&state.0, &parameters.0, inserter);
                S2::trace_slots(&state.1, &parameters.1, inserter);
            }
        }

        impl<S1, S2> IntoSignal for $signal_config<S1, S2>
        where
            S1: IntoSignal,
            S2: IntoSignal,
            $signal_name<S1::Signal, S2::Signal>: Signal<
                State = (IntoSignalState<S1>, IntoSignalState<S2>),
                Parameters = (IntoSignalParameters<S1>, IntoSignalParameters<S2>),
            >,
        {
            type Signal = $signal_name<S1::Signal, S2::Signal>;

            fn into_signal(self) -> IntoSignalResult<Self> {
                let l = self.0.into_signal()?;
                let r = self.1.into_signal()?;
                Ok(ReadySignal {
                    signal: $signal_name(l.signal, r.signal),
                    state: (l.state, r.state),
                    parameters: (l.parameters, r.parameters),
                })
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

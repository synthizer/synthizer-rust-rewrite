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
            A::Signal: for<'ol> Signal<Output<'ol> = IntoSignalOutput<'ol, B>>,
            for<'ol> IntoSignalOutput<'ol, A>: $trait<IntoSignalOutput<'ol, B>> + Copy,
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
            S2: for<'il> Signal<Input<'il> = SignalInput<'il, S1>>,
            for<'ol> SignalOutput<'ol, S1>: $trait<SignalOutput<'ol, S2>> + Copy,
            S1: 'static,
            S2: 'static,
        {
            type Input<'il> = SignalInput<'il, S1>;
            type Output<'ol> = <SignalOutput<'ol, S1> as $trait<SignalOutput<'ol, S2>>>::Output;
            type Parameters = (SignalParameters<S1>, SignalParameters<S2>);
            type State = (SignalState<S1>, SignalState<S2>);

            fn on_block_start(
                ctx: &SignalExecutionContext<'_, '_>,
                params: &Self::Parameters,
                state: &mut Self::State,
            ) {
                S1::on_block_start(ctx, &params.0, &mut state.0);
                S2::on_block_start(&ctx, &params.1, &mut state.1);
            }

            fn tick<'il, 'ol, D, const N: usize>(
                ctx: &'_ SignalExecutionContext<'_, '_>,
                input: [Self::Input<'il>; N],
                params: &Self::Parameters,
                state: &mut Self::State,
                mut destination: D,
            ) where
                Self::Input<'il>: 'ol,
                'il: 'ol,
                D: SignalDestination<Self::Output<'ol>, N>,
            {
                S1::tick::<_, N>(ctx, input.clone(), &params.0, &mut state.0, |left| {
                    S2::tick(ctx, input, &params.1, &mut state.1, |right| {
                        let outgoing = crate::array_utils::increasing_usize::<N>()
                            .map(|i| left[i].$method(right[i]));
                        destination.send(outgoing);
                    });
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

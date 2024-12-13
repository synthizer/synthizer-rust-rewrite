//! Implements the mathematical operations between `IntoSignal`s.
use std::ops::*;

use crate::chain::Chain;
use crate::core_traits::*;

pub struct AddSig<L, R>(L, R);
pub struct AddSigConfig<L, R>(L, R);

impl<A, B> Add<Chain<B>> for Chain<A>
where
    A: IntoSignal,
    B: IntoSignal,
    A::Signal: Signal<Output = IntoSignalOutput<B>>,
    IntoSignalOutput<A>: Add<IntoSignalOutput<B>>,
{
    type Output = Chain<AddSigConfig<A, B>>;

    fn add(self, rhs: Chain<B>) -> Self::Output {
        Chain {
            inner: AddSigConfig(self.inner, rhs.inner),
        }
    }
}

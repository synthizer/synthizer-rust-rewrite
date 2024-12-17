use crate::context::*;
/// Implementation of signals for scalars.
use crate::core_traits::*;
use crate::error::Result;

macro_rules! impl_scalar {
    ($t: ty) => {
        unsafe impl Signal for $t {
            type Input = ();
            type Output = $t;
            type State = ();
            type Parameters = $t;

            fn tick1<D: SignalDestination<Self::Output>>(
                ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
                _input: &'_ Self::Input,
                destination: D,
            ) {
                destination.send(*ctx.parameters);
            }

            fn on_block_start(_ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>) {}
        }

        impl IntoSignal for $t {
            type Signal = $t;

            fn into_signal(self) -> Result<ReadySignal<Self::Signal,IntoSignalState<Self>,IntoSignalParameters<Self>>> {
                Ok(ReadySignal {
                    signal:self,
                    state:(),
                    parameters:self,
                })
            }
        }
    };
}

impl_scalar!(i8);
impl_scalar!(i16);
impl_scalar!(i32);
impl_scalar!(i64);
impl_scalar!(u8);
impl_scalar!(u16);
impl_scalar!(u32);
impl_scalar!(u64);
impl_scalar!(usize);
impl_scalar!(isize);
impl_scalar!(f32);
impl_scalar!(f64);

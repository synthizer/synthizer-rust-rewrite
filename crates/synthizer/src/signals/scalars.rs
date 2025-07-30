use crate::context::*;
use crate::core_traits::*;

macro_rules! impl_scalar {
    ($t: ty) => {
        unsafe impl Signal for $t {
            type Input = ();
            type Output = $t;
            type State = $t;

            fn tick_frame(
                _ctx: &'_ SignalExecutionContext<'_, '_>,
                _input: Self::Input,
                state: &mut Self::State,
            ) -> Self::Output {
                *state
            }

            fn on_block_start(_ctx: &SignalExecutionContext<'_, '_>, _state: &mut Self::State) {}
        }

        impl IntoSignal for $t {
            type Signal = $t;

            fn into_signal(self) -> IntoSignalResult<Self> {
                Ok(ReadySignal {
                    signal: self,
                    state: self,
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

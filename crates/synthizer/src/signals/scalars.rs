use std::any::Any;
use std::sync::Arc;

use crate::context::*;
use crate::core_traits::*;
use crate::unique_id::UniqueId;

macro_rules! impl_scalar {
    ($t: ty) => {
        unsafe impl Signal for $t {
            type Input<'il> = ();
            type Output<'ol> = $t;
            type State = $t;

            fn tick<'il, 'ol, D, const N: usize>(
                _ctx: &'_ SignalExecutionContext<'_, '_>,
                input: [Self::Input<'il>; N],
                state: &mut Self::State,
                destination: D,
            ) where
                Self::Input<'il>: 'ol,
                'il: 'ol,
                D: SignalDestination<Self::Output<'ol>, N>,
            {
                destination.send(input.map(|_| *state));
            }

            fn on_block_start(_ctx: &SignalExecutionContext<'_, '_>, _state: &mut Self::State) {}

            fn trace_slots<F: FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>)>(
                _state: &Self::State,
                _inserter: &mut F,
            ) {
            }
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

use std::any::Any;
use std::sync::Arc;

use crate::context::*;
use crate::core_traits::*;
use crate::error::Result;
use crate::unique_id::UniqueId;

macro_rules! impl_scalar {
    ($t: ty) => {
        unsafe impl Signal for $t {
            type Input = ();
            type Output = $t;
            type State = ();
            type Parameters = $t;

            fn tick_block<
            'a,
            I: FnMut(usize) -> &'a Self::Input,
            D: ReusableSignalDestination<Self::Output>,
        >(
            ctx: &'_ mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
            mut input: I,
mut             destination: D,
        ) where
            Self::Input: 'a {
                for i in 0..crate::config::BLOCK_SIZE {
                    input(i);
                    destination.send_reusable(*ctx.parameters);
                }
            }

            fn on_block_start(_ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>) {}

            fn trace_slots<F: FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>)>(
                _state: &Self::State,
                _parameters: &Self::Parameters,
                _inserter: &mut F,
            ) {}
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

macro_rules! syzcall {
    ($i: ident) => {syzcall!($i,) };

    ($identifier: ident, $($args:expr),*) => {
        paste::paste! {
            crate::c_binding::[<syz_miniaudio_0_1_0_ $identifier>]($($args),*)
        }
    };
}

mod miniaudio_dispatch;
#[allow(warnings, clippy::all)]
mod c_binding;
mod device;
mod errors;
mod initialization;

pub use device::*;
pub use errors::*;

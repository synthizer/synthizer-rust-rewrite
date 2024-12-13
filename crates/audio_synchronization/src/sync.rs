#[cfg(not(loom))]
mod not_loom {
    pub use std::sync::atomic::*;
    pub use std::sync::*;
    // NOTE: loom doesn't have park_timeout.
    pub use std::thread::{current, park, Thread};

    pub use std::hint::spin_loop;

    #[cfg(test)]
    pub fn wrap_test(what: impl Fn() + Sync + Send + 'static) {
        what()
    }
}

#[cfg(not(loom))]
pub(crate) use not_loom::*;

#[cfg(loom)]
mod with_loom {
    pub use loom::sync::atomic::*;
    pub use loom::sync::*;
    pub use loom::thread::park;
    pub use loom::thread::{current, yield_now};
    pub use loom::thread::{spawn, JoinHandle, Thread};

    pub use loom::hint::spin_loop;

    #[cfg(test)]
    pub fn wrap_test(what: impl Fn() + Sync + Send + 'static) {
        loom::model(what);
    }
}

#[cfg(loom)]
pub(crate) use with_loom::*;

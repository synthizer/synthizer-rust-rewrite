#[cfg(not(loom))]
mod not_loom {
    pub use std::sync::atomic::*;
    pub use std::sync::*;
    pub use std::thread::spawn;
    pub use std::thread::yield_now;

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
    pub use loom::thread::spawn;
    pub use loom::thread::yield_now;

    #[cfg(test)]
    pub fn wrap_test(what: impl Fn() + Sync + Send + 'static) {
        loom::model(what)
    }
}
#[cfg(loom)]
pub(crate) use with_loom::*;

//! A fast way to get thread ids which can be converted to u64.
//!
//! This implementation:
//!
//! - Exposes `as_u64()`, which is currently nightly-only.
//! - Doesn't require having a handle to the current thread, which is the first part of how it can be faster, especially
//!   in debug builds.
//! - Like STD, ids are never reused (save if more than u64::MAX threads are spawned).
//! - Doesn't enter a CAS loop to verify u64 didn't wrap around (spawning u64::MAX threads is impossible in practice
//!   circa 2023), the second part of being faster, and also making the algorithm waitfree.
//! - Doesn't support targets without atomics (the algorithm is always lockfree).
//! - Is aware of loom if `--cfg=loom` is passed to the compiler.
//!
//! the catch is that if Cargo ever decides to include multiple versions of this crate, the ids are not guaranteed to be
//! the same type everywhere.  Don't expose [FastThreadId] in your public API.
use crate::sync::{AtomicU64, Ordering};

/// A thread id.
#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd, Debug, Hash)]
pub struct FastThreadId(u64);

impl std::fmt::Display for FastThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FastThreadId {
    #[inline]
    pub fn current() -> FastThreadId {
        #[cfg(not(loom))]
        static GLOBAL: AtomicU64 = AtomicU64::new(0);

        #[cfg(loom)]
        loom::lazy_static! {
            static ref  GLOBAL: AtomicU64 = AtomicU64::new(0);
        }

        #[cfg(loom)]
        loom::thread_local! {
            static LOCAL_ID: u64 = GLOBAL.fetch_add(1, Ordering::Relaxed);
        };

        #[cfg(not(loom))]
        std::thread_local! {
            static LOCAL_ID: u64 = GLOBAL.fetch_add(1, Ordering::Relaxed);
        };

        LOCAL_ID.with(|x| FastThreadId(*x))
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        crate::sync::wrap_test(|| {
            let mut jh: Vec<crate::sync::JoinHandle<FastThreadId>> = vec![];

            for _ in 0..3 {
                jh.push(crate::sync::spawn(FastThreadId::current));
            }

            let as_set = jh
                .into_iter()
                .map(|x| x.join().unwrap())
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(as_set.len(), 3);
        });
    }
}

mod hrtf;

include!(concat!(env!("OUT_DIR"), "/primes.rs"));

/// The maximum prime from [PRIMES].
pub const MAX_PRIME: u64 = PRIMES[PRIMES.len() - 1];

pub use hrtf::*;

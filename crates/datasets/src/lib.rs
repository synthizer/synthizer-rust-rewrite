include!(concat!(env!("OUT_DIR"), "/primes.rs"));

/// The maximum prime from [Primes].
pub const MAX_PRIME: u64 = PRIMES[PRIMES.len() - 1];

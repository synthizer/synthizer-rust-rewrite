use itertools::Itertools;
use primes::{PrimeSet, Sieve};
use std::io::Write;

/// What's the highest integer we need to approximate?
///
/// Goes one prime hier than this.
const MAX_PRIME_INT: u64 = 44100 * 10;

pub fn gen_primes() {
    let mut generated_primes = vec![];

    let mut sieve = Sieve::new();
    for i in sieve.iter() {
        generated_primes.push(i);
        if i > MAX_PRIME_INT {
            break;
        }
    }

    let type_name = format!("[u64; {}]", generated_primes.len());
    let literal = generated_primes.iter().join(",\n");
    let literal = format!("[\n{}\n]", literal);
    let max_prime = generated_primes.last().unwrap();

    let out = format!(
        r#"
/// An array of primes up to {max_prime}
/// 
/// This is useful for approximations.
/// 
/// Note that we expose the array as an array, not as a slice.  The size of the array may change.  Typical usage slices
/// this anyway, but the idea is that exposing it as an array can improve cross-crate optimizations by always letting
/// LLVM see through to the constant length.
pub const PRIMES: {type_name} = {literal};
"#
    );

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let final_path = format!("{out_dir}/primes.rs");
    let mut file = std::fs::File::create(final_path).unwrap();
    file.write_all(out.as_bytes()).unwrap();
}

//! Enhance normal match statements to fulfill the functions of loop unrolling/monomorphization.
//! 
//! Add `#[supermatch_fn]` to the top of a function.  Then add `#[supermatch]` to match statements in that function.
//! This will then perform the following transforms, provided that the pattern *exactly* matches:
//! 
//! - Any pattern of the form `x @ l..=h` where `l` and `h` are (possibly negated) integer literals is expanded to one
//!   arm for each integer.  `x` is redeclared as a const inside that arm, which can then be used with e.g. const
//!   generics, or simply to unroll loops.  One of the endpoints must have an integer suffix: `5u64`, etc.
//! - Any pattern of the form `a | b | c ...` is converted into one arm for each alternative.  The alternatives may
//!   consequently have different types: `Ok(x) | Err(x)` will expand for all `x`, as long as both arms compile given
//!   the type of the Ok and/or Err variant.
//! 
//! This *can* explode binary size.  Use with care.  The primary motivation is for Synthizer which needs to monomorphize
//! for common channel counts (e.g. stereo, 5.1 etc) as well as for some common sources of input.  nesting
//! `supermatch`-processed matches is `O(n^2)` on the number of arms at worst.  E.g. nesting `0..=15` inside `0..=15` is
//! 256 arms total.  it is important to note that binary size can equate to slowness: after a point (usually not
//! encountered in normal code) and provided that branches are unpredictable, the code can be too large for the CPU's
//! caches.
pub use supermatch_macro::supermatch_fn;

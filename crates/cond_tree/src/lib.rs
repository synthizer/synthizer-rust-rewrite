//! A crate for building trees of if statements with duplicate bodies.
//!
//! When doing mathematical coding in tight loops, sometimes you have a structure like this:
//!
//! ```IGNORE
//! for i in whatever {
//!     if expensive_condition {
//!         do_expensive_thing(i);
//!     } else {
//!         do_cheap_thing(i);
//!     }
//! }
//! ```
//!
//! The traditional way to make this fast, assuming the compiler doesn't, is to invert the loop so that the condition is
//! on the outside.  This generally exposes autovectorization opportunities, but the cost is having duplicate copies of
//! the loop body with very slight variations.  Fortunately, the common case of one or two conditions and a tight loop
//! is generally optimized for you.
//!
//! But sometimes, there's many more conditions.  Consider for example crossfading 5 inputs to an audio effect if they
//! change, where the output depends on feedback etc.  It would be nice to be able to write the loop the normal way
//! rather than duplicating it 20 times.  This crate exposes a macro to do so.
//!
//! There are two entities involved.  [Divergence] is a trait which encapsulates over things which are "like
//! conditions".  This includes bools, results, etc.  Then there is [Cond], an enum representing the result of
//! evaluating a [Divergence].  [Cond] has two cases, `Slow` and `Fast`, which are the values of evaluating the
//! condition and may be of different types.  The provided macro then matches on [Cond]s to "unroll" the tree of
//! conditions.  
mod maybe_int;

pub use maybe_int::*;

/// The result of evaluating a [Divergence].
#[derive(derive_more::IsVariant, derive_more::Unwrap)]
pub enum Cond<F, S> {
    /// The "slow" path.  `Cond`s can be collapsed into their slow cases, if the slow side impls `From<R>`.
    Slow(S),

    /// The fast case.
    Fast(F),
}

/// A divergence.
///
/// This trait represents the evaluation of something which diverges.  For example, a bool diverges to the values `true`
/// and `false, a result to `(Ok(fast)` and Err(slow)`, etc.
///
/// This crate provides the following implementations:
///
/// - [bool]: `true` is the fast path.
/// - [Result]: [Result::Ok] is the fast path.
/// - [Cond]: identity, to provide a good customization point.
/// - [MaybeInt]: a divergence which becomes a constant if a given value matches a compile-time-provided value.  Useful
///       for example when working with strided array accesses where the stride is often one.
///
///
/// This crate also provides an implementation for tuples of conds, in order to allow building more complex trees where
/// some conditions are correlated.  This is useful because the tree is O(n^2), so if there are some subset of those
/// that strongly correlate a tuple can be used instead.  This implementation collapses to the slow variant of all
/// elements of the tuple if any is slow, otherwise it resolves to the fast (that is, fast=all are fast, otherwise
/// slow).  This is done by using the `From` impl.
///
/// One unintuitive element of the tuple variation is that if the tuple diverges to two arms of the same type, the from
/// impl will "flip" them around.  For example, a `Cond<u32, u32>` will copy the fast path to the slow variant if
/// converted to slow because `u32: From<u32>` returns the given value.  The primary intent of the tuple variation is to
/// handle diverging types, for example two kinds of delay line reading where one performs modulus.  That is:
///
/// ```IGNORE
/// struct FastReader;
/// struct SlowReader;
///
/// impl From<FastReader> for SlowReader { ... }
///
/// fn get_a_divergence() -> Cond<SlowReader, FastReader> { ... }
/// ```
///
/// Or similar.
///
/// It is necessary to be able to convert some types into owned variants, so implementors should decide whether to
/// implement the trait on references or values or both.
pub trait Divergence {
    type Slow;
    type Fast;

    /// Evaluate the divergence, returning the side to use.
    fn evaluate_divergence(self) -> Cond<Self::Fast, Self::Slow>;
}

/// A type representing true.
///
/// [TrueTy::get] returns true, and conversions from [FalseTy] invert the bool.
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueTy;

impl TrueTy {
    pub const fn get(&self) -> bool {
        true
    }
}

/// A type representing false.
///
/// [FalseTy::get] returns false, and conversions from [TrueTy] invert the bool to be false.
#[derive(Copy, Clone, Debug, Default)]
pub struct FalseTy;

impl FalseTy {
    pub const fn get(&self) -> bool {
        false
    }
}

macro_rules! bool_from {
    ($T1:ty, $T2:ty) => {
        impl From<$T2> for $T1 {
            fn from(_input: $T2) -> $T1 {
                Default::default()
            }
        }
    };
}

bool_from!(TrueTy, FalseTy);
bool_from!(FalseTy, TrueTy);

impl Divergence for bool {
    type Slow = FalseTy;
    type Fast = TrueTy;

    fn evaluate_divergence(self) -> Cond<Self::Fast, Self::Slow> {
        if self {
            Cond::Fast(TrueTy)
        } else {
            Cond::Slow(FalseTy)
        }
    }
}

impl<T, E> Divergence for Result<T, E> {
    type Slow = E;
    type Fast = T;

    fn evaluate_divergence(self) -> Cond<T, E> {
        match self {
            Ok(x) => Cond::Fast(x),
            Err(e) => Cond::Slow(e),
        }
    }
}

impl<'a, T, E> Divergence for &'a Result<T, E> {
    type Slow = &'a E;
    type Fast = &'a T;

    fn evaluate_divergence(self) -> Cond<Self::Fast, Self::Slow> {
        match self {
            Ok(ref x) => Cond::Fast(x),
            Err(ref e) => Cond::Slow(e),
        }
    }
}

impl<F, S> Divergence for Cond<F, S> {
    type Slow = S;
    type Fast = F;

    fn evaluate_divergence(self) -> Cond<F, S> {
        self
    }
}

impl<'a, F, S> Divergence for &'a Cond<F, S> {
    type Slow = &'a S;
    type Fast = &'a F;

    fn evaluate_divergence(self) -> Cond<&'a F, &'a S> {
        match self {
            Cond::Slow(l) => Cond::Slow(l),
            Cond::Fast(r) => Cond::Fast(r),
        }
    }
}

impl<F, S> Cond<F, S> {
    pub fn new(divergence: impl Divergence<Slow = S, Fast = F>) -> Self {
        divergence.evaluate_divergence()
    }

    /// Converte to a [Cond] of units.
    ///
    /// This is useful when what matters is the side, but not the value.
    pub fn to_unit(&self) -> Cond<(), ()> {
        match self {
            Cond::Fast(_) => Cond::Fast(()),
            Cond::Slow(_) => Cond::Slow(()),
        }
    }
}

impl<F, S> Cond<F, S>
where
    S: From<F>,
{
    fn become_slow(self) -> Cond<F, S> {
        match self {
            Cond::Fast(f) => Cond::Slow(f.into()),
            Cond::Slow(s) => Cond::Slow(s),
        }
    }
}

impl<S, F> PartialEq for Cond<S, F>
where
    S: Eq,
    F: Eq,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Cond::Fast(a), Cond::Fast(b)) => a == b,
            (Cond::Slow(a), Cond::Slow(b)) => a == b,
            _ => false,
        }
    }
}

impl<S, F> Eq for Cond<S, F>
where
    S: Eq,
    F: Eq,
{
}

impl<S: std::fmt::Debug, F: std::fmt::Debug> std::fmt::Debug for Cond<S, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = if self.is_fast() {
            f.debug_tuple("Cond::Fast")
        } else {
            f.debug_tuple("Cond::Slow")
        };

        match self {
            Cond::Fast(f) => {
                s.field(f);
            }
            Cond::Slow(x) => {
                s.field(x);
            }
        }

        s.finish()
    }
}

cond_tree_macro::cond_tree_macro_tuples!(32);

/// Expand a set of conditions into a tree, morally equivalent to:
///
/// ```IGNORE
/// if cond1 {
///     if cond2 {
///         body
///     } else {
///         body
///     }
/// } else {
///     if cond2 {
///         body
///     } else {
///         body
///     }
/// }
/// ```
///
/// Except that it is possible to inject named values.  For example (see below for the meanings of the patterns):
///
/// ```IGNORE
/// cond_tree!(
///     let thing1 = a_divergence,
///     let thing2 = if condition { expr1} else { expr2 },
///     (let thing3 = ..., let thing4 = ..., let thing5 = ...),
///     => {
///         println!("{}",thing1);
///         println!("{}", thing2);
///     }
/// )
/// ```
///
/// Each input to the macro before the => defines a divergent condition.  The block after the => is then duplicated for
/// all permutations.  The accepted patterns are as follows:
///
/// - `identifier`: assumes that `identifier` implements [Divergence], evaluates the identifier, and shadows it with an
///   identifier of the same name.  If the [Divergence] impl returns different types, the block must be valid for both.
/// - `let identifier = expr` (but not an expr of the form `if cond {...} else {...}`): assumes that the result of
///   `expr` implements [Divergence], and then uses that.
/// - `let identifier = if cond { ... } else { ... }`: `identifier` will be the first expression if `cond` is true,
///   otherwise the second.  The first expression is assumed to be the fast path, e.g. [Cond::Fast].
///
/// All of the above may have an optional type `let a: (fast, slow) = ...`.  `let` may also be `const`, but if and only
/// if the two expressions will evaluate to constants at compile time, and the types are not optional nor can they be
/// `_`.  in effect this means `const` can only be used with the `if ...` form.
#[macro_export]
macro_rules! cond_tree {
    ($pats: tt => $b: block) => {{
        // Set up the scope.
        use $crate::Cond;
        use $crate::Divergence;

        cond_tree_macro::cond_tree_macro_impl!($pats => $b)
    }};
}

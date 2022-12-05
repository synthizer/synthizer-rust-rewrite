use std::marker::PhantomData;

use crate::{Cond, Divergence};

/// A divergence which can resolve to a given constant given the common case of that constant.
///
/// This is used like `MaybeInt<usize, 1>::new(actual_value)` and resolves to either [FixedInt] or [VaryingInt], both of
/// which have a `get()` function to return the value.  When resolving to [FixedInt], inlining can usually get rid of
/// any dynamicism because, even though Rust limitations prevent us making that function const at the type level, it
/// simply returns a constant value directly.
///
/// To see the usefulness, consider a mathematical kernel:
///
/// ```IGNORE
/// fn convolve(input: &[T], input_stride: usize, ...) {
///     ...
/// }
/// ```
///
/// If we know that the stride is often 1 and the slice is long enough to make possibly unpredictable branches worth it,
/// then we might use `MaybeInt<usize, 1>`: this will allow for optimizing the trivial case in which there is no stride,
/// which can usually be trivially vectorized.
///
/// Unfortunately, Rust const generics doesn't allow const args to depend on other type parameters.  For this reason,
/// the const arg in the generics is always i64, and a panic will result if `new()` is called on an instantiation which
/// cannot fit in a `T`.  Note that while `u64` is supported, a fundamental limitation here is that the maximum integral
/// value may only be `i64::MAX`.  For now, we don't support i/u128.
///
/// The list of supported types is all sizes of integer, signed or unsigned, up to 64 bits.  This is enforced via a
/// sealed trait.
#[derive(Copy, Clone)]
pub struct MaybeInt<T: IntType, const COMMON: i64>(T);

// Num isn't zero-overhead because they check signedness etc. so we must, sadly, implement a sealed trait ourselves.
mod sealed {
    pub trait IntType: Copy + Eq {
        const MIN: i64;
        const MAX: i64;

        fn from_i64(what: i64) -> Self;
    }

    macro_rules! simple {
        ($t: tt) => {
            impl IntType for $t {
                const MIN: i64 = $t::MIN as i64;
                const MAX: i64 = $t::MAX as i64;

                fn from_i64(what: i64) -> $t {
                    what as $t
                }
            }
        };
    }

    simple!(i8);
    simple!(u8);
    simple!(i16);
    simple!(u16);
    simple!(i32);
    simple!(u32);
    simple!(i64);

    // but u64 is special, because the range is based on i64.
    impl IntType for u64 {
        const MIN: i64 = 0;
        const MAX: i64 = i64::MAX;

        fn from_i64(what: i64) -> u64 {
            what as u64
        }
    }
}

use sealed::*;

impl<T: IntType, const COMMON: i64> MaybeInt<T, COMMON> {
    pub fn new(value: T) -> MaybeInt<T, COMMON> {
        assert!((T::MIN..=T::MAX).contains(&COMMON));
        MaybeInt(value)
    }
}

/// The fast half of the [MaybeInt] divergence, when the constant given in [MaybeInt::new] matches the constant in the
/// generics.
#[derive(Copy, Clone)]
pub struct FixedInt<T: IntType, const COMMON: i64>(PhantomData<T>);

/// the slow side of the [MaybeInt] divergence, when the constant doesn't match.
#[derive(Copy, Clone)]
pub struct VaryingInt<T: IntType>(T);

impl<T: IntType, const COMMON: i64> FixedInt<T, COMMON> {
    #[inline(always)]
    pub fn get(&self) -> T {
        T::from_i64(COMMON)
    }

    /// Returns whether this is the fixed variant of the [MaybeInt] divergence.
    ///
    /// Since this is [FixedInt], always returns true.
    #[inline(always)]
    pub const fn is_fixed(&self) -> bool {
        true
    }
}

impl<T: IntType> VaryingInt<T> {
    #[inline(always)]
    pub fn get(&self) -> T {
        self.0
    }

    /// Returns whether this is the fixed variant of the [MaybeInt] divergence.
    ///
    /// Since this is [VaryingInt], always returns false.
    #[inline(always)]
    pub const fn is_fixed(&self) -> bool {
        false
    }
}

impl<T: IntType, const COMMON: i64> Divergence for MaybeInt<T, COMMON> {
    type Fast = FixedInt<T, COMMON>;
    type Slow = VaryingInt<T>;

    fn evaluate_divergence(self) -> Cond<Self::Fast, Self::Slow> {
        if self.0 == T::from_i64(COMMON) {
            Cond::Fast(FixedInt::<T, COMMON>(PhantomData))
        } else {
            Cond::Slow(VaryingInt(self.0))
        }
    }
}

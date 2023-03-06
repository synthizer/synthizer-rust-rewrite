//! A module encapsulating potentially compile-time integers.
//!
//! This is typically used with [supermatch]: we carve out a range of common values with range patterns, then have the
//! catch-all case be dynamic.

use std::marker::PhantomData;

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

    impl IntType for usize {
        const MIN: i64 = 0;
        const MAX: i64 = {
            if (usize::MAX as u64) < i64::MAX as u64 {
                usize::MAX as i64
            } else {
                i64::MAX
            }
        };

        fn from_i64(what: i64) -> usize {
            what as usize
        }
    }
}

use sealed::*;

/// A compile-time-known integer.
///
/// this is used with [supermatch] to monomorphize over common integers.  See [IntSource].
#[derive(Copy, Clone)]
pub struct FixedInt<T: IntType, const VAL: i64>(PhantomData<T>);

/// A runtime-known integer.
///
/// this is used with [supermatch] to monomorphize over common integers.
///
/// See [IntSource].
#[derive(Copy, Clone)]
pub struct VaryingInt<T: IntType>(pub T);

impl<T: IntType, const VAL: i64> FixedInt<T, VAL> {
    pub fn new() -> Self {
        Self(PhantomData)
    }

    #[inline(always)]
    pub fn get(&self) -> T {
        T::from_i64(VAL)
    }

    #[inline(always)]
    pub const fn is_fixed(&self) -> bool {
        true
    }
}

impl<T: IntType> VaryingInt<T> {
    pub fn new(val: T) -> Self {
        Self(val)
    }

    #[inline(always)]
    pub fn get(&self) -> T {
        self.0
    }

    #[inline(always)]
    pub const fn is_fixed(&self) -> bool {
        false
    }
}

pub trait IntSource {
    type Output;

    fn as_int(&self) -> Self::Output;
}

impl<T: IntType, const VAL: i64> IntSource for FixedInt<T, VAL> {
    type Output = T;

    fn as_int(&self) -> Self::Output {
        self.get()
    }
}

impl<T: IntType> IntSource for VaryingInt<T> {
    type Output = T;

    fn as_int(&self) -> Self::Output {
        self.get()
    }
}

// Original xoroshiro license: Written in 2019 by David Blackman and Sebastiano Vigna (vigna@acm.org)
//
//To the extent possible under law, the author has dedicated all copyright and related and neighboring rights to this
// software to the public domain worldwide. This software is distributed without any warranty.
//
// See <http://creativecommons.org/publicdomain/zero/1.0/>.

/// A fast xoroshiro generator, generating up to `N` numbers at once.
///
/// This is like e.g. `rand_xoroshiro` except it has a lot of unrolling, and it doesn't implement the full API because
/// that's unnecessary for audio use cases, which work fine with random integers and floats between -1.0 and 1.0.
///
/// implements: https://xoshiro.di.unimi.it/xoroshiro128plusplus.c
#[derive(Clone, Debug)]
pub struct FastXoroshiro128PlusPlus<const N: usize> {
    s0: [u64; N],
    s1: [u64; N],
}

// Let's force inlining. To do so, use a bunch of macros instead of functions.

macro_rules! next {
    ($self:expr, $ind: expr) => {{
        let s0 = $self.s0[$ind];
        let mut s1 = $self.s1[$ind];

        let result: u64 = s0.wrapping_add(s1).rotate_left(17).wrapping_add(s0);
        s1 ^= s0;
        $self.s0[$ind] = s0.rotate_left(49) ^ s1 ^ (s1 << 21);
        $self.s1[$ind] = s1.rotate_left(28);
        result
    }};
}

impl<const N: usize> FastXoroshiro128PlusPlus<N> {
    pub fn new_seeded(seed: u64) -> Self {
        use rand::prelude::*;
        use rand_xoshiro::SplitMix64;
        let mut sm64 = SplitMix64::seed_from_u64(seed);
        let mut s0: [u64; N] = [0; N];
        let mut s1: [u64; N] = [0; N];

        for i in 0..N {
            s0[i] = sm64.random();
            s1[i] = sm64.random();
        }
        Self { s0, s1 }
    }

    /// generate a single random u64.
    #[inline(always)]
    pub fn gen_u64(&mut self) -> u64 {
        self.gen_array::<1>()[0]
    }

    /// Fill a slice of u64 with random values.
    ///
    /// This is significantly faster than `next`, if the slice is large enough, and faster still if the slice is a multiple of `n`.
    #[inline]
    pub fn gen_slice(&mut self, destination: &mut [u64]) {
        let full_iters = destination.len() / N;
        for i in 0..full_iters {
            let arr: &mut [u64; N] = (&mut destination[i * N..(i + 1) * N]).try_into().unwrap();
            *arr = self.gen_array();
        }

        #[allow(clippy::needless_range_loop)]
        for i in (full_iters * N)..destination.len() {
            let gen = i % N;
            destination[i] = next!(self, gen);
        }
    }

    /// Return an array of random values.
    ///
    /// This is significantly faster than `next` for larger arrays, and even faster still if the requested array size is a multiple of `N`.
    #[inline]
    pub fn gen_array<const O: usize>(&mut self) -> [u64; O] {
        let mut out = [0; O];
        let full = O / N;
        for i in 0..full {
            for gen in 0..N {
                out[i * N + gen] = next!(self, gen);
            }
        }

        #[allow(clippy::needless_range_loop)]
        for i in (full * N)..O {
            let gen = i % N;
            out[i] = next!(self, gen);
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next() {
        let mut gen = FastXoroshiro128PlusPlus::<4>::new_seeded(5);
        assert_eq!(
            [gen.gen_u64(), gen.gen_u64(), gen.gen_u64()],
            [
                4303094124001495694,
                16928758989761721026,
                14664196110570592231
            ]
        );
    }

    #[test]
    fn test_generating_arrays_exact_size() {
        let mut gen = FastXoroshiro128PlusPlus::<4>::new_seeded(5);
        let old_s0 = gen.s0;
        let old_s1 = gen.s1;
        let exact_size: [u64; 4] = gen.gen_array();
        assert_eq!(
            exact_size,
            [
                4303094124001495694,
                9293363455363148617,
                12366563110907314400,
                15902818224793000299
            ]
        );

        assert_ne!(gen.s0, old_s0);
        assert_ne!(gen.s1, old_s1);

        for (i, (s0, s1)) in old_s0
            .iter()
            .copied()
            .zip(old_s1.iter().copied())
            .enumerate()
        {
            assert_ne!(gen.s0[i], s0, "at index {}", i);
            assert_ne!(gen.s1[i], s1, "at index {}", i);
        }
    }

    #[test]
    fn test_generating_arrays_multiple_of_n() {
        let mut gen = FastXoroshiro128PlusPlus::<4>::new_seeded(5);
        let old_s0 = gen.s0;
        let old_s1 = gen.s1;
        let multiple: [u64; 8] = gen.gen_array();
        assert_eq!(
            multiple,
            [
                4303094124001495694,
                9293363455363148617,
                12366563110907314400,
                15902818224793000299,
                16928758989761721026,
                2656217596593880698,
                14072207190013699828,
                8948144846666717681
            ],
        );

        assert_ne!(gen.s0, old_s0);
        assert_ne!(gen.s1, old_s1);

        for (i, (s0, s1)) in old_s0
            .iter()
            .copied()
            .zip(old_s1.iter().copied())
            .enumerate()
        {
            assert_ne!(gen.s0[i], s0, "at index {}", i);
            assert_ne!(gen.s1[i], s1, "at index {}", i);
        }
    }

    #[test]
    fn test_generating_arrays_less_than_n() {
        let mut gen = FastXoroshiro128PlusPlus::<4>::new_seeded(5);
        let old_s0 = gen.s0;
        let old_s1 = gen.s1;
        let lesser: [u64; 3] = gen.gen_array();
        assert_eq!(
            lesser,
            [
                4303094124001495694,
                9293363455363148617,
                12366563110907314400,
            ],
        );

        assert_ne!(gen.s0, old_s0);
        assert_ne!(gen.s1, old_s1);

        for (i, (s0, s1)) in old_s0
            .iter()
            .copied()
            .zip(old_s1.iter().copied())
            .enumerate()
            .take(3)
        {
            assert_ne!(gen.s0[i], s0, "at index {}", i);
            assert_ne!(gen.s1[i], s1, "at index {}", i);
        }
    }

    #[test]
    fn test_generating_arrays_with_remainder() {
        let mut gen = FastXoroshiro128PlusPlus::<4>::new_seeded(5);
        let old_s0 = gen.s0;
        let old_s1 = gen.s1;
        let with_remainder: [u64; 7] = gen.gen_array();
        assert_eq!(
            with_remainder,
            [
                4303094124001495694,
                9293363455363148617,
                12366563110907314400,
                15902818224793000299,
                16928758989761721026,
                2656217596593880698,
                14072207190013699828
            ],
        );

        assert_ne!(gen.s0, old_s0);
        assert_ne!(gen.s1, old_s1);

        for (i, (s0, s1)) in old_s0
            .iter()
            .copied()
            .zip(old_s1.iter().copied())
            .enumerate()
            .take(3)
        {
            assert_ne!(gen.s0[i], s0, "at index {}", i);
            assert_ne!(gen.s1[i], s1, "at index {}", i);
        }
    }

    #[test]
    fn test_generating_slices_exact_size() {
        let mut gen = FastXoroshiro128PlusPlus::<4>::new_seeded(5);
        let old_s0 = gen.s0;
        let old_s1 = gen.s1;
        let mut exact_size: [u64; 4] = [0; 4];
        gen.gen_slice(&mut exact_size[..]);
        assert_eq!(
            exact_size,
            [
                4303094124001495694,
                9293363455363148617,
                12366563110907314400,
                15902818224793000299
            ]
        );

        assert_ne!(gen.s0, old_s0);
        assert_ne!(gen.s1, old_s1);

        for (i, (s0, s1)) in old_s0
            .iter()
            .copied()
            .zip(old_s1.iter().copied())
            .enumerate()
        {
            assert_ne!(gen.s0[i], s0, "at index {}", i);
            assert_ne!(gen.s1[i], s1, "at index {}", i);
        }
    }

    #[test]
    fn test_generating_slices_multiple_of_n() {
        let mut gen = FastXoroshiro128PlusPlus::<4>::new_seeded(5);
        let old_s0 = gen.s0;
        let old_s1 = gen.s1;
        let mut multiple: [u64; 8] = [0; 8];
        gen.gen_slice(&mut multiple[..]);
        assert_eq!(
            multiple,
            [
                4303094124001495694,
                9293363455363148617,
                12366563110907314400,
                15902818224793000299,
                16928758989761721026,
                2656217596593880698,
                14072207190013699828,
                8948144846666717681
            ],
        );

        assert_ne!(gen.s0, old_s0);
        assert_ne!(gen.s1, old_s1);

        for (i, (s0, s1)) in old_s0
            .iter()
            .copied()
            .zip(old_s1.iter().copied())
            .enumerate()
        {
            assert_ne!(gen.s0[i], s0, "at index {}", i);
            assert_ne!(gen.s1[i], s1, "at index {}", i);
        }
    }

    #[test]
    fn test_generating_slices_less_than_n() {
        let mut gen = FastXoroshiro128PlusPlus::<4>::new_seeded(5);
        let old_s0 = gen.s0;
        let old_s1 = gen.s1;
        let mut lesser: [u64; 3] = [0; 3];
        gen.gen_slice(&mut lesser[..]);
        assert_eq!(
            lesser,
            [
                4303094124001495694,
                9293363455363148617,
                12366563110907314400,
            ],
        );

        assert_ne!(gen.s0, old_s0);
        assert_ne!(gen.s1, old_s1);

        for (i, (s0, s1)) in old_s0
            .iter()
            .copied()
            .zip(old_s1.iter().copied())
            .enumerate()
            .take(3)
        {
            assert_ne!(gen.s0[i], s0, "at index {}", i);
            assert_ne!(gen.s1[i], s1, "at index {}", i);
        }
    }

    #[test]
    fn test_generating_slices_with_remainder() {
        let mut gen = FastXoroshiro128PlusPlus::<4>::new_seeded(5);
        let old_s0 = gen.s0;
        let old_s1 = gen.s1;
        let mut with_remainder: [u64; 7] = [0; 7];
        gen.gen_slice(&mut with_remainder[..]);
        assert_eq!(
            with_remainder,
            [
                4303094124001495694,
                9293363455363148617,
                12366563110907314400,
                15902818224793000299,
                16928758989761721026,
                2656217596593880698,
                14072207190013699828
            ],
        );

        assert_ne!(gen.s0, old_s0);
        assert_ne!(gen.s1, old_s1);

        for (i, (s0, s1)) in old_s0
            .iter()
            .copied()
            .zip(old_s1.iter().copied())
            .enumerate()
            .take(3)
        {
            assert_ne!(gen.s0[i], s0, "at index {}", i);
            assert_ne!(gen.s1[i], s1, "at index {}", i);
        }
    }
}

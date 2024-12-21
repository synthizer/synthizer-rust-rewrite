use std::mem::MaybeUninit;
/// Return an array `[0, 1, ...]`.
pub(crate) fn increasing_usize<const N: usize>() -> [usize; N] {
    let mut ret = [0; N];
    #[allow(clippy::needless_range_loop)] // false positive because we cannot collect into arrays.
    for i in 0..N {
        ret[i] = i;
    }
    ret
}

pub(crate) fn collect_iter<I, const N: usize>(iterator: I) -> [I::Item; N]
where
    I: Iterator,
{
    let mut ret: [MaybeUninit<I::Item>; N] = [const { MaybeUninit::uninit() }; N];
    let did = ret
        .iter_mut()
        .zip(iterator)
        .map(|(a, b)| a.write(b))
        .count();
    assert_eq!(did, N);
    unsafe { ret.map(|x| x.assume_init()) }
}

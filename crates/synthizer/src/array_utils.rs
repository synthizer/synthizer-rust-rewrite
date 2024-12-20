/// Return an array `[0, 1, ...]`.
pub(crate) fn increasing_usize<const N: usize>() -> [usize; N] {
    let mut ret = [0; N];
    for i in 0..N {
        ret[i] = i;
    }
    ret
}

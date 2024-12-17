use std::mem::MaybeUninit;

/// Drop an array of MaybeUninit.
///
/// # Safety
///
/// The array must have actually been fully initialized.
pub(crate) unsafe fn drop_initialized_array<T, const N: usize>(mut x: [MaybeUninit<T>; N]) {
    x.iter_mut().for_each(|x| x.assume_init_drop());
}

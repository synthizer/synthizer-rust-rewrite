//! Simple threshold-based floating point asserts.
//!
//! We could grab various crates for this but we generally want thresholds bigger than epsilon, and this is both small
//! and simple.

#[track_caller]
pub(crate) fn close_floats32(a: f32, b: f32, threshold: f32) {
    let diff = (a - b).abs();
    assert!(
        diff < threshold,
        "{} vs {}, difference {} is greater than threshold {}",
        a,
        b,
        diff,
        threshold
    );
}

#[track_caller]
pub(crate) fn close_floats64(a: f64, b: f64, threshold: f64) {
    let diff = (a - b).abs();
    assert!(
        diff < threshold,
        "{} vs {}, difference {} is greater than threshold {}",
        a,
        b,
        diff,
        threshold
    );
}

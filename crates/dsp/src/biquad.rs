//! Biquad filters, primarily from the [Audio Eq Cookbook](https://www.w3.org/TR/audio-eq-cookbook/).
//!
//! This modulel also offers first-order sections by setting the later coefficients in the biquad to 0.
use std::f64::consts::PI;

use num::complex::Complex64;

use crate::config::*;

/// Suggested default for the `Q` parameter.
///
/// This makes the audio eq filters (in particular lowpass and highpass) second-order butterworth sections.
pub const DEFAULT_Q: f64 = 0.7071135624381276;

/// A 1-channnel biquad filter.
///
/// Implements the transfer function `(b0 + b1 z^-1 + b2 z^-2) / (a0 + a1 z^-1 + a2 z^-2)`, factored so that `a0` and
/// `b0` are always 1 and pulled out into a gain factor.
#[derive(Debug, Clone)]
pub struct MonoBiquadFilter {
    def: BiquadFilterDef,

    // The history.  The first element of the history is implicit: it comes from the current computation
    history: [f64; 2],
}

/// A definition for a biquad filter.
#[derive(Debug, Clone)]
pub struct BiquadFilterDef {
    gain: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl MonoBiquadFilter {
    pub fn new(def: BiquadFilterDef) -> MonoBiquadFilter {
        MonoBiquadFilter {
            def,
            history: [0.0; 2],
        }
    }

    /// Tick this filter by 1 sample.
    pub fn tick(&mut self, input_sample: f64) -> f64 {
        // direct form 2: do the recursive part first, then convolve the numerator using only the denominator history rather than having two histories.
        //
        // Put the gain in first.
        let with_gain = input_sample * self.def.gain;
        let recursive = with_gain + self.def.a1 * self.history[0] + self.def.a2 * self.history[1];
        let out = recursive + self.def.b1 * self.history[0] + self.def.b2 * self.history[1];
        self.history.swap(0, 1);
        out
    }
}

// Some helpers which compute common variables from the Audio EQ cookbook.
fn bq_omega0(freq: f64) -> f64 {
    2.0 * PI * freq / (SR as f64)
}

fn bq_a(dbgain: f64) -> f64 {
    10.0f64.powf(dbgain / 40.0)
}

fn bq_alpha_q(omega0: f64, q: f64) -> f64 {
    omega0.sin() / (2.0 * q)
}

fn bq_alpha_bw(omega0: f64, bw: f64) -> f64 {
    omega0.sin() * (2.0f64.log2() * bw * omega0 / (2.0 * omega0.sin())).sinh()
}

fn bq_alpha_s(omega0: f64, s: f64, a: f64) -> f64 {
    let mul1 = a + 1.0f64 / a;
    let mul2 = 1.0f64 / s + 1.0f64;
    let sqrt = (mul1 * mul2 + 2.0).sqrt();
    omega0.sin() / 2.0 * sqrt
}

/// Kinds of thing which can be used for defining the "Q" of a filter.
///
/// The Audio EQ cookbook defines 3 possibilities, `Q`, `BW` and `S`.  All filters can take `BW` and `Q`, but only
/// lowshelf/highshelf and peaking should take `S`; if `S` is used with other filter types, a panic results.
///
/// Note that the unit for `Bw` is octaves.  To get a bandwidth for a specific frequency and range, use [AudioEqAlpha::bw_from_hz].
#[derive(Debug, Copy, Clone)]
pub enum AudioEqAlpha {
    Q(f64),
    Bw(f64),
    S(f64),
}

impl AudioEqAlpha {
    fn compute_alpha(&self, omega0: f64, a: Option<f64>) -> f64 {
        match self {
            Self::Q(q) => bq_alpha_q(omega0, *q),
            Self::Bw(bw) => bq_alpha_bw(omega0, *bw),
            Self::S(s) => bq_alpha_s(
                omega0,
                *s,
                a.expect("This filter type does not support S as a parameter"),
            ),
        }
    }

    /// Get a [AudioEqAlpha::Bw] for a given midpoint and interval, that is `midpoint - interval` to `midpoint +
    /// interval`, or `2 * interval` bandwidth.
    pub fn bw_from_hz(midpoint: f64, interval: f64) -> AudioEqAlpha {
        let min = midpoint - interval;
        let octaves = interval * 2.0 / min;

        Self::Bw(octaves)
    }
}

impl BiquadFilterDef {
    pub fn new_raw(b: [f64; 3], a: [f64; 3]) -> Self {
        let gain = b[0] / a[0];

        let b1 = b[1] / b[0];
        let b2 = b[2] / b[0];
        let a1 = a[1] / a[0];
        let a2 = a[2] / a[0];
        Self {
            gain,
            b1,
            b2,
            a1,
            a2,
        }
    }

    /// Lowpass Audio Eq Biquad, specifying frequency in hz.
    pub fn audio_eq_lowpass(frequency: f64, alpha: AudioEqAlpha) -> Self {
        let omega0 = bq_omega0(frequency);
        let b1 = 1.0 - omega0.cos();
        let b0 = b1 / 2.0f64;
        let b2 = b0;
        let alpha = alpha.compute_alpha(omega0, None);
        let a0 = 1.0f64 + alpha;
        let a1 = -2.0 * omega0.cos();
        let a2 = 1.0 - alpha;
        Self::new_raw([b0, b1, b2], [a0, a1, a2])
    }

    /// The highpass filter from the Audio Eq Cookbook
    pub fn audio_eq_highpass(frequency: f64, alpha: AudioEqAlpha) -> Self {
        let omega0 = bq_omega0(frequency);
        let shared = 1.0f64 + omega0.cos();
        let b0 = shared / 2.0;
        let b1 = -shared;
        let b2 = b0;
        let alpha = alpha.compute_alpha(omega0, None);
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * omega0.cos();
        let a2 = 1.0 - alpha;
        Self::new_raw([b0, b1, b2], [a0, a1, a2])
    }

    /// Audio EQ cookbook BPF filter of constant skirt gain.
    ///
    /// note that, though this is a bandpass, the peak gain is [AudioEqAlpha::Q] and bandwidth ends up not making sense.
    /// `Q(gain)` is the gain at the specified frequency exactly, as a raw multiplier on the input.
    pub fn audio_eq_bandpass_constant_skirt(frequency: f64, alpha: AudioEqAlpha) -> Self {
        let omega0 = bq_omega0(frequency);
        let alpha = alpha.compute_alpha(omega0, None);
        let b0 = omega0.sin() / 2.0;
        let b1 = 0.0f64;
        let b2 = -b0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * omega0.cos();
        let a2 = 1.0 - alpha;
        Self::new_raw([b0, b1, b2], [a0, a1, a2])
    }

    /// Audio EQ bandpass filter where peak gain is 0.
    pub fn audio_eq_bandpass_peak_0(frequency: f64, alpha: AudioEqAlpha) -> Self {
        let omega0 = bq_omega0(frequency);
        let alpha = alpha.compute_alpha(omega0, None);
        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * omega0.cos();
        let a2 = 1.0 - alpha;
        Self::new_raw([b0, b1, b2], [a0, a1, a2])
    }

    /// The aAudio EQ Cookbook's notch filter.
    pub fn audio_eq_notch(frequency: f64, alpha: AudioEqAlpha) -> Self {
        let omega0 = bq_omega0(frequency);
        let alpha = alpha.compute_alpha(omega0, None);
        let b0 = 1.0f64;
        let b1 = -2.0 * omega0.cos();
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * omega0.cos();
        let a2 = 1.0 - alpha;
        Self::new_raw([b0, b1, b2], [a0, a1, a2])
    }

    /// the Audio EQ Cookbook's allpass filter.
    pub fn audio_eq_allpass(frequency: f64, alpha: AudioEqAlpha) -> Self {
        let omega0 = bq_omega0(frequency);
        let alpha = alpha.compute_alpha(omega0, None);
        let b0 = 1.0 - alpha;
        let b1 = -2.0 * omega0.cos();
        let b2 = 1.0 + alpha;
        let a0 = b2;
        let a1 = b1;
        let a2 = 1.0 - alpha;
        Self::new_raw([b0, b1, b2], [a0, a1, a2])
    }

    /// The Audio Eq Peaking EQ filter.
    ///
    /// This filter accepts [AudioEqAlpha::S].
    pub fn audio_eq_peaking(frequency: f64, dbgain: f64, alpha: AudioEqAlpha) -> Self {
        let omega0 = bq_omega0(frequency);
        let a = bq_a(dbgain);
        let alpha = alpha.compute_alpha(omega0, Some(a));
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * omega0.cos();
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = b1;
        let a2 = b2;
        Self::new_raw([b0, b1, b2], [a0, a1, a2])
    }

    /// The Audio Eq Lowshelf.
    ///
    /// this filter accepts [AudioEqAlpha::S].
    pub fn audio_eq_lowshelf(frequency: f64, dbgain: f64, alpha: AudioEqAlpha) -> Self {
        let omega0 = bq_omega0(frequency);
        let a = bq_a(dbgain);
        let alpha = alpha.compute_alpha(omega0, Some(a));
        let b0 = a * ((a + 1.0) - (a - 1.0) * omega0.cos() + 2.0 * a.sqrt() * alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * omega0.cos());
        let b2 = a * ((a + 1.0) - (a - 1.0) * omega0.cos() - 2.0 * a.sqrt() * alpha);
        let a0 = (a + 1.0) - (a - 1.0) * omega0.cos() + 2.0 * a.sqrt() * alpha;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * omega0.cos());
        let a2 = (a + 1.0) - (a - 1.0) * omega0.cos() - 2.0 * a.sqrt() * alpha;
        Self::new_raw([b0, b1, b2], [a0, a1, a2])
    }

    /// The Audio EQ Highshelf
    ///
    /// This filter takes [AudioEqAlpha::S].
    pub fn audio_eq_highshelf(frequency: f64, dbgain: f64, alpha: AudioEqAlpha) -> Self {
        let omega0 = bq_omega0(frequency);
        let a = bq_a(dbgain);
        let alpha = alpha.compute_alpha(omega0, Some(a));
        let b0 = a * ((a + 1.0) + (a - 1.0) * omega0.cos() + 2.0 * a.sqrt() * alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * omega0.cos());
        let b2 = a * ((a + 1.0) - (a - 1.0) * omega0.cos() - 2.0 * a.sqrt() * alpha);
        let a0 = (a + 1.0) - (a - 1.0) * omega0.cos() + 2.0 * a.sqrt() * alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * omega0.cos());
        let a2 = (a + 1.0) - (a - 1.0) * omega0.cos() - 2.0 * a.sqrt() * alpha;
        Self::new_raw([b0, b1, b2], [a0, a1, a2])
    }

    /// Get the frequency response of this filter as a complex number, given a frequency in hZ.
    pub fn frequency_response(&self, frequency: f64) -> Complex64 {
        let omega = bq_omega0(frequency);
        let z_inv = 1.0 / (Complex64::i() * omega).exp();

        self.gain * (1.0 + self.b1 * z_inv + self.b2 * z_inv.powu(2))
            / (1.0 + self.a1 * z_inv + self.a2 * z_inv.powu(2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{close_floats::*, DbExt};

    #[test]
    fn test_lowpass_design() {
        let filt = BiquadFilterDef::audio_eq_lowpass(10000.0, AudioEqAlpha::Q(DEFAULT_Q));
        close_floats64(
            filt.frequency_response(10000.0).norm().gain_to_db(),
            -3.0,
            0.02,
        );

        close_floats64(
            filt.frequency_response(1000.0).norm().gain_to_db(),
            0.0,
            0.02,
        );

        close_floats64(
            filt.frequency_response(20000.0).norm().gain_to_db(),
            -35.84,
            0.02,
        );
    }

    #[test]
    fn test_highpass_design() {
        let filt = BiquadFilterDef::audio_eq_highpass(10000.0, AudioEqAlpha::Q(DEFAULT_Q));
        close_floats64(
            filt.frequency_response(10000.0).norm().gain_to_db(),
            -3.0,
            0.02,
        );

        close_floats64(
            filt.frequency_response(20000.0).norm().gain_to_db(),
            0.0,
            0.02,
        );

        close_floats64(
            filt.frequency_response(1000.0).norm().gain_to_db(),
            -43.31,
            0.02,
        );
    }

    #[test]
    fn test_bandpass_peak_zero() {
        let filt = BiquadFilterDef::audio_eq_bandpass_peak_0(
            10000.0,
            AudioEqAlpha::bw_from_hz(10000.0, 1000.0),
        );
        close_floats64(
            filt.frequency_response(10000.0).norm().gain_to_db(),
            0.0,
            0.001,
        );
        close_floats64(
            filt.frequency_response(8900.0).norm().gain_to_db(),
            -3.0,
            0.04,
        );
        close_floats64(
            filt.frequency_response(11100.0).norm().gain_to_db(),
            -2.93,
            0.04,
        );

        // Now we want to check a couple very far away, for sanity.
        close_floats64(
            filt.frequency_response(1000.0).norm().gain_to_db(),
            -31.46,
            0.04,
        );
        close_floats64(
            filt.frequency_response(20000.0).norm().gain_to_db(),
            -27.64,
            0.04,
        );
    }

    #[test]
    fn test_bandpass_constant_skirt() {
        let filt = BiquadFilterDef::audio_eq_bandpass_constant_skirt(10000.0, AudioEqAlpha::Q(2.0));
        close_floats64(
            filt.frequency_response(10000.0).norm().gain_to_db(),
            6.0,
            0.03,
        );
        close_floats64(
            filt.frequency_response(8900.0).norm().gain_to_db(),
            4.503,
            0.04,
        );
        close_floats64(
            filt.frequency_response(11100.0).norm().gain_to_db(),
            4.56,
            0.04,
        );

        // Now we want to check a couple very far away, for sanity.
        close_floats64(
            filt.frequency_response(1000.0).norm().gain_to_db(),
            -21.606,
            0.04,
        );
        close_floats64(
            filt.frequency_response(20000.0).norm().gain_to_db(),
            -17.79,
            0.04,
        );
    }

    #[test]
    fn test_allpass() {
        let filt = BiquadFilterDef::audio_eq_allpass(10000.0, AudioEqAlpha::Q(2.0));
        close_floats64(
            filt.frequency_response(10000.0).norm().gain_to_db(),
            0.0,
            0.001,
        );
        close_floats64(
            filt.frequency_response(8900.0).norm().gain_to_db(),
            0.0,
            0.04,
        );
        close_floats64(
            filt.frequency_response(11100.0).norm().gain_to_db(),
            0.0,
            0.04,
        );
    }

    #[test]
    fn test_highshelf() {
        let filt = BiquadFilterDef::audio_eq_highshelf(10000.0, 3.0, AudioEqAlpha::S(1.0));
        close_floats64(
            filt.frequency_response(10000.0).norm().gain_to_db(),
            1.589,
            0.001,
        );
        close_floats64(
            filt.frequency_response(8900.0).norm().gain_to_db(),
            1.42,
            0.04,
        );
        close_floats64(
            filt.frequency_response(11100.0).norm().gain_to_db(),
            1.74,
            0.04,
        );
        close_floats64(
            filt.frequency_response(20000.0).norm().gain_to_db(),
            2.83,
            0.04,
        );
    }

    #[test]
    fn test_lowshelf() {
        let filt = BiquadFilterDef::audio_eq_lowshelf(10000.0, 3.0, AudioEqAlpha::S(1.0));
        close_floats64(
            filt.frequency_response(10000.0).norm().gain_to_db(),
            1.498,
            0.001,
        );
        close_floats64(
            filt.frequency_response(8900.0).norm().gain_to_db(),
            1.66,
            0.04,
        );
        close_floats64(
            filt.frequency_response(11100.0).norm().gain_to_db(),
            1.34,
            0.04,
        );
        close_floats64(
            filt.frequency_response(20000.0).norm().gain_to_db(),
            0.25,
            0.04,
        );
        close_floats64(
            filt.frequency_response(1000.0).norm().gain_to_db(),
            3.229,
            0.003,
        );
    }
}

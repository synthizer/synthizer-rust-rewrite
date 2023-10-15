use crate::config::SR;

#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct SinEval;

#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct CosEval;

#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct TanEval;

#[derive(Copy, Clone, Eq, Ord, PartialEq, PartialOrd)]
enum TrigFunction {
    Sin(SinEval),
    Cos(CosEval),
    Tan(TanEval),
}

macro_rules! eval_impl {
    ($struct_ident:ident, $tf: ident) => {
        impl $struct_ident {
            fn eval(&self, offset: f64) -> f64 {
                (offset * 2.0 * std::f64::consts::PI).$tf()
            }
        }
    };
}

eval_impl!(SinEval, sin);
eval_impl!(CosEval, cos);
eval_impl!(TanEval, tan);

/// A trigonometric waveform at a given frequency.
pub(crate) struct TrigWaveformEvaluator {
    evaluator: TrigFunction,

    /// From 0.0 to 1.0.  The offset of the wave.
    phase: f64,

    frequency: f64,
}

macro_rules! constructor {
    ($name:ident, $tvariant: expr) => {
        pub(crate) fn $name(frequency: f64, phase: f64) -> Self {
            Self {
                evaluator: $tvariant,
                frequency,
                phase,
            }
        }
    };
}

impl TrigWaveformEvaluator {
    constructor!(new_sin, TrigFunction::Sin(SinEval));
    constructor!(new_cos, TrigFunction::Cos(CosEval));
    constructor!(new_tan, TrigFunction::Tan(TanEval));

    pub(crate) fn set_frequency(&mut self, frequency: f64) -> &mut Self {
        self.frequency = frequency;
        self
    }

    pub(crate) fn set_phase(&mut self, phase: f64) -> &mut Self {
        self.phase = phase;
        self
    }

    /// Tick this trigonometric waveform some number of times, pushing output values as well as a tick count to the
    /// provided closure.
    #[supermatch::supermatch_fn]
    pub(crate) fn evaluate_ticks(&mut self, ticks: usize, mut destination: impl FnMut(usize, f64)) {
        let increment = self.frequency / SR as f64;

        #[supermatch]
        match self.evaluator {
            TrigFunction::Sin(e) | TrigFunction::Cos(e) | TrigFunction::Tan(e) => {
                for i in 0..ticks {
                    let this_phase = self.phase + i as f64 * increment;
                    let res = e.eval(this_phase);
                    destination(i, res);
                }

                self.phase += increment * ticks as f64;
                self.phase = self.phase.rem_euclid(1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sin() {
        const SAMPLES: usize = 20 * crate::config::BLOCK_SIZE;
        const FREQ: f64 = 300.0;

        let expected = (0..SAMPLES)
            .map(|x| {
                (x as f64 * 2.0 * std::f64::consts::PI * FREQ / crate::config::SR as f64).sin()
            })
            .collect::<Vec<f64>>();

        let mut got = vec![0.0f64; SAMPLES];

        let mut evaluator = TrigWaveformEvaluator::new_sin(FREQ, 0.0);

        for i in 0..(SAMPLES / crate::config::BLOCK_SIZE) {
            let start = i * crate::config::BLOCK_SIZE;
            let slice = &mut got[start..start + crate::config::BLOCK_SIZE];
            evaluator.evaluate_ticks(crate::config::BLOCK_SIZE, |i, dest| {
                slice[i] = dest;
            });
        }

        for (i, (g, e)) in got.into_iter().zip(expected.into_iter()).enumerate() {
            assert!(
                (g - e).abs() < 0.01,
                "Sample {i} is too different: got={g}, expected={e}",
            );
        }
    }
}

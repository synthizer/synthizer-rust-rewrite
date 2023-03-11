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
            fn eval(&self, offset: f32) -> f32 {
                (offset * 2.0 * std::f32::consts::PI).$tf()
            }
        }
    };
}

eval_impl!(SinEval, sin);
eval_impl!(CosEval, cos);
eval_impl!(TanEval, tan);

/// A trigonometric waveform at a given frequency.
pub(crate) struct TrigWaveform {
    evaluator: TrigFunction,

    /// From 0.0 to 1.0.  The offset of the wave.
    phase: f32,

    frequency: f32,
}

macro_rules! constructor {
    ($name:ident, $tvariant: expr) => {
        pub(crate) fn $name(frequency: f32, phase: f32) -> Self {
            Self {
                evaluator: $tvariant,
                frequency,
                phase,
            }
        }
    };
}

impl TrigWaveform {
    constructor!(new_sin, TrigFunction::Sin(SinEval));
    constructor!(new_cos, TrigFunction::Cos(CosEval));
    constructor!(new_tan, TrigFunction::Tan(TanEval));

    fn set_frequency(&mut self, frequency: f32) -> &mut Self {
        self.frequency = frequency;
        self
    }

    pub fn set_phase(&mut self, phase: f32) -> &mut Self {
        self.phase = phase;
        self
    }

    /// Tick this trigonometric waveform some number of times, pushing output values as well as a tick count to the
    /// provided closure.
    #[supermatch::supermatch_fn]
    fn evaluate_ticks(&mut self, ticks: usize, mut destination: impl FnMut(usize, f32)) {
        let increment = self.frequency / SR as f32;

        #[supermatch]
        match self.evaluator {
            TrigFunction::Sin(e) | TrigFunction::Cos(e) | TrigFunction::Tan(e) => {
                for i in 0..ticks {
                    let this_phase = self.phase + i as f32 * increment;
                    let res = e.eval(this_phase);
                    destination(i, res);
                }

                self.phase += increment * ticks as f32;
                self.phase = self.phase.rem_euclid(1.0);
            }
        }
    }
}

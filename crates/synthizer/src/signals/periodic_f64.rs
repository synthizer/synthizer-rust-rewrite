use crate::context::*;
use crate::core_traits::*;
use crate::error::Result;

/// A signal which produces an f64 value in the range (0..period) by summing the value of an input signal. e.g.
/// modulation is allowed.  The period must be fixed.
///
/// This is not the same as doing this as two separate steps, because the internal value is properly reset every time
/// the period is hit.
pub struct PeriodicF64Config<SIncrCfg> {
    pub(crate) frequency: SIncrCfg,
    pub(crate) period: f64,
    pub(crate) initial_value: f64,
}

pub struct PeriodicF64Signal<SIncr>(SIncr);

pub struct PeriodicF64State<SIncr: Signal> {
    freq_state: SIncr::State,
    cur_val: f64,
    period: f64,
}

fn inc1<SIncr: Signal>(state: &mut PeriodicF64State<SIncr>, increment: f64) -> f64 {
    let cur_val = state.cur_val;
    let new_val = (cur_val + increment) % state.period;
    state.cur_val = new_val;
    cur_val
}

unsafe impl<SIncr> Signal for PeriodicF64Signal<SIncr>
where
    SIncr: Signal<Output = f64>,
{
    type Output = f64;
    type Input = SIncr::Input;
    type State = PeriodicF64State<SIncr>;

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        SIncr::on_block_start(ctx, &mut state.freq_state);
    }

    fn tick_frame(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let increment = SIncr::tick_frame(ctx, input, &mut state.freq_state);
        inc1(state, increment)
    }
}

impl<SIncrCfg> IntoSignal for PeriodicF64Config<SIncrCfg>
where
    SIncrCfg: IntoSignal,
    SIncrCfg::Signal: Signal<Output = f64>,
{
    type Signal = PeriodicF64Signal<SIncrCfg::Signal>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let inner = self.frequency.into_signal()?;
        Ok(ReadySignal {
            signal: PeriodicF64Signal(inner.signal),
            state: PeriodicF64State {
                freq_state: inner.state,
                cur_val: self.initial_value,
                period: self.period,
            },
        })
    }

    fn trace<F: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        inserter: &mut F,
    ) -> Result<()> {
        self.frequency.trace(inserter)?;
        Ok(())
    }
}

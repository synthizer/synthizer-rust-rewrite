use crate::context::*;
use crate::core_traits::*;

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
}

pub struct PeriodicF64Parameters<SIncr: Signal> {
    period: f64,
    freq_params: SIncr::Parameters,
}

unsafe impl<SIncr> Signal for PeriodicF64Signal<SIncr>
where
    SIncr: Signal<Output = f64>,
{
    type Output = f64;
    type Input = SIncr::Input;
    type State = PeriodicF64State<SIncr>;
    type Parameters = PeriodicF64Parameters<SIncr>;

    fn tick1<D: SignalDestination<Self::Output>>(
        ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        input: &'_ Self::Input,
        destination: D,
    ) {
        let period = ctx.parameters.period;
        let mut val = ctx.state.cur_val;

        SIncr::tick1(
            &mut ctx.wrap(|s| &mut s.freq_state, |p| &p.freq_params),
            input,
            |incr| {
                // If we do not send the value first, then the first value sent is never 0.0 (or the otherwise specified
                // initial value).
                destination.send(val);
                val += incr;
                val %= period;
            },
        );

        ctx.state.cur_val = val;
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
            },
            parameters: PeriodicF64Parameters {
                freq_params: inner.parameters,
                period: self.period,
            },
        })
    }
}

use crate::config;
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

fn inc1<SIncr: Signal>(
    state: &mut PeriodicF64State<SIncr>,
    params: &PeriodicF64Parameters<SIncr>,
    increment: f64,
) -> f64 {
    let cur_val = state.cur_val;
    let new_val = (cur_val + increment) % params.period;
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
    type Parameters = PeriodicF64Parameters<SIncr>;

    fn tick1<D: SignalDestination<Self::Output>>(
        ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        input: &'_ Self::Input,
        destination: D,
    ) {
        let mut parent: f64 = 0.0;
        SIncr::tick1(
            &mut ctx.wrap(|s| &mut s.freq_state, |p| &p.freq_params),
            input,
            |incr| {
                parent = incr;
            },
        );

        destination.send(inc1(ctx.state, ctx.parameters, parent));
    }

    fn on_block_start(ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>) {
        SIncr::on_block_start(&mut ctx.wrap(|s| &mut s.freq_state, |p| &p.freq_params));
    }

    fn tick_block<
        'a,
        I: FnMut(usize) -> &'a Self::Input,
        D: ReusableSignalDestination<Self::Output>,
    >(
        ctx: &'_ mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        input: I,
        mut destination: D,
    ) where
        Self::Input: 'a,
    {
        let mut increments: [f64; config::BLOCK_SIZE] = [0.0; config::BLOCK_SIZE];
        let mut i = 0;
        SIncr::tick_block(
            &mut ctx.wrap(|s| &mut s.freq_state, |p| &p.freq_params),
            input,
            |x| {
                increments[i] = x;
                i += 1;
            },
        );

        increments.into_iter().for_each(|val| {
            destination.send_reusable(inc1(ctx.state, ctx.parameters, val));
        });
    }

    fn trace_slots<
        F: FnMut(
            crate::unique_id::UniqueId,
            std::sync::Arc<dyn std::any::Any + Send + Sync + 'static>,
        ),
    >(
        state: &Self::State,
        parameters: &Self::Parameters,
        inserter: &mut F,
    ) {
        SIncr::trace_slots(&state.freq_state, &parameters.freq_params, inserter);
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

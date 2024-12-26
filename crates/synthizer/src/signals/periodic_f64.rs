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
    SIncr: for<'a> Signal<Output<'a> = f64>,
{
    type Output<'ol> = f64;
    type Input<'il> = SIncr::Input<'il>;
    type State = PeriodicF64State<SIncr>;

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        SIncr::on_block_start(ctx, &mut state.freq_state);
    }

    fn tick<'il, 'ol, I, const N: usize>(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: I,
        state: &mut Self::State,
    ) -> impl ValueProvider<Self::Output<'ol>>
    where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        I: ValueProvider<Self::Input<'il>> + Sized,
    {
        let par = SIncr::tick::<_, N>(ctx, input, &mut state.freq_state);
        let increments = crate::array_utils::collect_iter::<_, N>(unsafe { par.become_iterator() });

        let results = increments.map(|x| inc1(state, x));

        ArrayProvider::<_, N>::new(results)
    }

    fn trace_slots<
        F: FnMut(
            crate::unique_id::UniqueId,
            std::sync::Arc<dyn std::any::Any + Send + Sync + 'static>,
        ),
    >(
        state: &Self::State,
        inserter: &mut F,
    ) {
        SIncr::trace_slots(&state.freq_state, inserter);
    }
}

impl<SIncrCfg> IntoSignal for PeriodicF64Config<SIncrCfg>
where
    SIncrCfg: IntoSignal,
    SIncrCfg::Signal: for<'ol> Signal<Output<'ol> = f64>,
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
}

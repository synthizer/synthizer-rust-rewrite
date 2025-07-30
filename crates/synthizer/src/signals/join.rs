use crate::core_traits::*;

pub struct JoinSignalConfig<ParSigCfg1, ParSigCfg2>(ParSigCfg1, ParSigCfg2);
pub struct JoinSignal<ParSig1, ParSig2>(ParSig1, ParSig2);
pub struct JoinSignalState<ParSigState1, ParSigState2>(ParSigState1, ParSigState2);

unsafe impl<ParSig1, ParSig2> Signal for JoinSignal<ParSig1, ParSig2>
where
    ParSig1: Signal,
    ParSig2: Signal,
{
    type Input = (SignalInput<ParSig1>, SignalInput<ParSig2>);
    type Output = (SignalOutput<ParSig1>, SignalOutput<ParSig2>);
    type State = JoinSignalState<SignalState<ParSig1>, SignalState<ParSig2>>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        ParSig1::on_block_start(ctx, &mut state.0);
        ParSig2::on_block_start(ctx, &mut state.1);
    }

    fn tick_frame(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let (left_input, right_input) = input;
        let left_output = ParSig1::tick_frame(ctx, left_input, &mut state.0);
        let right_output = ParSig2::tick_frame(ctx, right_input, &mut state.1);
        (left_output, right_output)
    }
}

impl<ParSigCfg1, ParSigCfg2> IntoSignal for JoinSignalConfig<ParSigCfg1, ParSigCfg2>
where
    JoinSignal<ParSigCfg1::Signal, ParSigCfg2::Signal>:
        Signal<State = JoinSignalState<IntoSignalState<ParSigCfg1>, IntoSignalState<ParSigCfg2>>>,
    ParSigCfg1: IntoSignal,
    ParSigCfg2: IntoSignal,
{
    type Signal = JoinSignal<ParSigCfg1::Signal, ParSigCfg2::Signal>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let par_l = self.0.into_signal()?;
        let par_r = self.1.into_signal()?;

        Ok(ReadySignal {
            signal: JoinSignal(par_l.signal, par_r.signal),
            state: JoinSignalState(par_l.state, par_r.state),
        })
    }
}

impl<ParSigCfg1, ParSigCfg2> JoinSignalConfig<ParSigCfg1, ParSigCfg2> {
    pub(crate) fn new(par_sig1: ParSigCfg1, par_sig2: ParSigCfg2) -> Self {
        Self(par_sig1, par_sig2)
    }
}

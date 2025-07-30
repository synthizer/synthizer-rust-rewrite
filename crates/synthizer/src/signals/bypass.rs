use crate::core_traits::*;

/// A signal which splits off the output from the upstream signal, passes it as the input to the bypass signal, and
/// returns a tuple containing both results.
///
/// This is used to get the value both before and after some effect; see [crate::Chain::bypass].
pub struct BypassSignalConfig<ParSigCfg, BypassSigCfg> {
    parent_cfg: ParSigCfg,
    bypass_cfg: BypassSigCfg,
}

pub struct BypassSignal<ParSig, BypassSig> {
    parent_sig: ParSig,
    bypass_sig: BypassSig,
}

pub struct BypassSignalState<ParSigState, BypassSigState> {
    parent_sig_state: ParSigState,
    bypass_sig_state: BypassSigState,
}

unsafe impl<ParSig, BypassSig> Signal for BypassSignal<ParSig, BypassSig>
where
    ParSig: Signal,
    BypassSig: Signal<Input = ParSig::Output>,
    ParSig::Output: Clone,
{
    type Input = ParSig::Input;
    type Output = (ParSig::Output, BypassSig::Output);
    type State = BypassSignalState<ParSig::State, BypassSig::State>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        ParSig::on_block_start(ctx, &mut state.parent_sig_state);
        BypassSig::on_block_start(ctx, &mut state.bypass_sig_state);
    }

    fn tick_frame(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let parent_output = ParSig::tick_frame(ctx, input, &mut state.parent_sig_state);
        let bypass_output =
            BypassSig::tick_frame(ctx, parent_output.clone(), &mut state.bypass_sig_state);
        (parent_output, bypass_output)
    }
}

impl<ParSigCfg, BypassSigCfg> IntoSignal for BypassSignalConfig<ParSigCfg, BypassSigCfg>
where
    ParSigCfg: IntoSignal,
    BypassSigCfg: IntoSignal,
    BypassSignal<ParSigCfg::Signal, BypassSigCfg::Signal>: Signal<
        State = BypassSignalState<IntoSignalState<ParSigCfg>, IntoSignalState<BypassSigCfg>>,
    >,
{
    type Signal = BypassSignal<ParSigCfg::Signal, BypassSigCfg::Signal>;

    fn into_signal(self) -> crate::Result<ReadySignal<Self::Signal, IntoSignalState<Self>>> {
        let par = self.parent_cfg.into_signal()?;
        let byp = self.bypass_cfg.into_signal()?;
        Ok(ReadySignal {
            state: BypassSignalState {
                parent_sig_state: par.state,
                bypass_sig_state: byp.state,
            },
            signal: BypassSignal {
                parent_sig: par.signal,
                bypass_sig: byp.signal,
            },
        })
    }
}

impl<ParSigCfg, BypassSigCfg> BypassSignalConfig<ParSigCfg, BypassSigCfg> {
    pub fn new(parent: ParSigCfg, bypass: BypassSigCfg) -> Self {
        Self {
            parent_cfg: parent,
            bypass_cfg: bypass,
        }
    }
}

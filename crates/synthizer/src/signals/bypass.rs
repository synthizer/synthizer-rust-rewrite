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
    BypassSig::Output: Clone,
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

    fn tick<I, const N: usize>(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: I,
        state: &mut Self::State,
    ) -> impl ValueProvider<Self::Output>
    where
        I: ValueProvider<Self::Input> + Sized,
    {
        let orig = crate::array_utils::collect_iter::<_, N>(
            ParSig::tick::<_, N>(ctx, input, &mut state.parent_sig_state).iter_cloned(),
        );
        let bypassed = BypassSig::tick::<_, N>(
            ctx,
            ArrayProvider::new(orig.clone()),
            &mut state.bypass_sig_state,
        );
        let res = orig.into_iter().zip(bypassed.iter_cloned());
        ArrayProvider::<_, N>::new(crate::array_utils::collect_iter::<_, N>(res))
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

    fn trace<F: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        inserter: &mut F,
    ) -> crate::Result<()> {
        self.parent_cfg.trace(inserter)?;
        self.bypass_cfg.trace(inserter)?;
        Ok(())
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

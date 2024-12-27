use std::mem::MaybeUninit;

use crate::core_traits::*;

pub struct JoinSignalConfig<ParSigCfg1, ParSigCfg2>(ParSigCfg1, ParSigCfg2);
pub struct JoinSignal<ParSig1, ParSig2>(ParSig1, ParSig2);
pub struct JoinSignalState<ParSigState1, ParSigState2>(ParSigState1, ParSigState2);

unsafe impl<ParSig1, ParSig2> Signal for JoinSignal<ParSig1, ParSig2>
where
    ParSig1: Signal,
    ParSig2: Signal,
{
    type Input<'il> = (SignalInput<'il, ParSig1>, SignalInput<'il, ParSig2>);
    type Output<'ol> = (SignalOutput<'ol, ParSig1>, SignalOutput<'ol, ParSig2>);
    type State = JoinSignalState<SignalState<ParSig1>, SignalState<ParSig2>>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        ParSig1::on_block_start(ctx, &mut state.0);
        ParSig2::on_block_start(ctx, &mut state.1);
    }

    fn tick<'il, 'ol, I, const N: usize>(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: I,
        state: &mut Self::State,
    ) -> impl ValueProvider<Self::Output<'ol>>
    where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        I: ValueProvider<Self::Input<'il>> + Sized,
    {
        let mut left_in: [MaybeUninit<SignalInput<'il, ParSig1>>; N] =
            [const { MaybeUninit::uninit() }; N];
        let mut right_in: [MaybeUninit<SignalInput<'il, ParSig2>>; N] =
            [const { MaybeUninit::uninit() }; N];

        let mut last_i = 0;
        for (i, (l, r)) in unsafe { input.become_iterator() }.enumerate() {
            left_in[i].write(l);
            right_in[i].write(r);
            last_i = i;
        }

        assert_eq!(last_i, N - 1);

        let par_left_out = ParSig1::tick::<_, N>(
            ctx,
            ArrayProvider::new(left_in.map(|x| unsafe { x.assume_init() })),
            &mut state.0,
        );
        let par_right_out = ParSig2::tick::<_, N>(
            ctx,
            ArrayProvider::new(right_in.map(|x| unsafe { x.assume_init() })),
            &mut state.1,
        );

        let outgoing = crate::array_utils::collect_iter::<_, N>(
            unsafe { par_left_out.become_iterator() }
                .zip(unsafe { par_right_out.become_iterator() }),
        );
        ArrayProvider::new(outgoing)
    }

    fn trace_slots<
        F: FnMut(
            crate::unique_id::UniqueId,
            std::sync::Arc<dyn std::any::Any + Send + Sync + 'static>,
        ),
    >(
        state: &Self::State,
        mut inserter: &mut F,
    ) {
        ParSig1::trace_slots(&state.0, &mut inserter);
        ParSig2::trace_slots(&state.1, &mut inserter);
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

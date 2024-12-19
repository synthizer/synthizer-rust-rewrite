use std::marker::PhantomData as PD;
use std::mem::MaybeUninit;

use crate::core_traits::*;

pub struct MapSignal<ParSig, F, O>(PD<*const (ParSig, F, O)>);
unsafe impl<ParSig, F, O> Send for MapSignal<ParSig, F, O> {}
unsafe impl<ParSig, F, O> Sync for MapSignal<ParSig, F, O> {}

pub struct MapSignalConfig<ParSigCfg, F, O> {
    parent: ParSigCfg,
    closure: F,
    _phantom: PD<O>,
}

pub struct MapSignalState<ParSig: Signal, F> {
    closure: F,

    parent_state: SignalState<ParSig>,
}

unsafe impl<ParSig, F, O> Signal for MapSignal<ParSig, F, O>
where
    ParSig: Signal,
    F: FnMut(&SignalOutput<ParSig>) -> O + Send + Sync + 'static,
    O: Send,
{
    type Input = SignalInput<ParSig>;
    type Output = O;
    type Parameters = ParSig::Parameters;
    type State = MapSignalState<ParSig, F>;

    fn on_block_start(
        ctx: &mut crate::context::SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
    ) {
        ParSig::on_block_start(&mut ctx.wrap(|s| &mut s.parent_state, |p| p));
    }

    fn tick<
        'a,
        I: FnMut(usize) -> &'a Self::Input,
        D: ReusableSignalDestination<Self::Output>,
        const N: usize,
    >(
        ctx: &'_ mut crate::context::SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        input: I,
        mut destination: D,
    ) where
        Self::Input: 'a,
    {
        let mut outs: [MaybeUninit<SignalOutput<ParSig>>; N] = [const { MaybeUninit::uninit() }; N];
        let mut i = 0;
        ParSig::tick::<_, _, N>(&mut ctx.wrap(|s| &mut s.parent_state, |p| p), input, |x| {
            outs[i].write(x);
            i += 1;
        });

        outs.iter().for_each(|i| {
            destination.send_reusable((ctx.state.closure)(unsafe { i.assume_init_ref() }))
        });

        // The mapping closure gets references, so we must drop this ourselves.
        unsafe {
            crate::unsafe_utils::drop_initialized_array(outs);
        }
    }

    fn trace_slots<
        Tracer: FnMut(
            crate::unique_id::UniqueId,
            std::sync::Arc<dyn std::any::Any + Send + Sync + 'static>,
        ),
    >(
        state: &Self::State,
        parameters: &Self::Parameters,
        inserter: &mut Tracer,
    ) {
        ParSig::trace_slots(&state.parent_state, parameters, inserter);
    }
}

impl<ParSig, F, O> IntoSignal for MapSignalConfig<ParSig, F, O>
where
    F: FnMut(&IntoSignalOutput<ParSig>) -> O + Send + Sync + 'static,
    ParSig: IntoSignal,
    O: Send + 'static,
{
    type Signal = MapSignal<ParSig::Signal, F, O>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let par = self.parent.into_signal()?;

        Ok(ReadySignal {
            parameters: par.parameters,
            state: MapSignalState {
                closure: self.closure,
                parent_state: par.state,
            },
            signal: MapSignal(PD),
        })
    }
}

impl<ParSig, F, O> MapSignalConfig<ParSig, F, O> {
    pub(crate) fn new(parent: ParSig, closure: F) -> Self {
        Self {
            closure,
            parent,
            _phantom: PD,
        }
    }
}

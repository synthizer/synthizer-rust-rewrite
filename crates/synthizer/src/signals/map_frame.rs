use std::marker::PhantomData as PD;

use crate::core_traits::*;

/// Signal which maps a closure over frames
///
/// This is useful for example when doing math ops with constants and a frame, e.g. volume adjustment.
///
/// See [crate::Chain::map_frame] for more documentation.
pub struct MapFrameSignalConfig<T, F, ParSigCfg> {
    parent_config: ParSigCfg,
    closure: F,
    _phantom: PD<T>,
}

pub struct MapFrameSignal<T, F, ParSig> {
    parent: ParSig,
    _phantom: PD<(T, F)>,
}

unsafe impl<T, F, ParSig> Send for MapFrameSignal<T, F, ParSig> {}
unsafe impl<T, F, ParSig> Sync for MapFrameSignal<T, F, ParSig> {}

pub struct MapFrameSignalState<ParState, F> {
    closure: F,
    par_state: ParState,
}

unsafe impl<T, F, ParSig> Signal for MapFrameSignal<T, F, ParSig>
where
    F: for<'ol> FnMut(usize, &T) -> T + Send + Sync + 'static,
    ParSig: Signal,
    for<'ol> ParSig::Output<'ol>: AudioFrame<T>,
    T: Copy + Default + 'static,
{
    type Input<'il> = ParSig::Input<'il>;
    type Output<'ol> = ParSig::Output<'ol>;
    type State = MapFrameSignalState<ParSig::State, F>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        ParSig::on_block_start(ctx, &mut state.par_state);
    }

    fn tick<'il, 'ol, 's, I, const N: usize>(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: I,
        state: &'s mut Self::State,
    ) -> impl ValueProvider<Self::Output<'ol>>
    where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        's: 'ol,
        I: ValueProvider<Self::Input<'il>> + Sized,
    {
        let par_provider = ParSig::tick::<_, N>(ctx, input, &mut state.par_state);
        let res_iter = par_provider.iter_cloned().map(|mut frame| {
            for i in 0..frame.channel_count() {
                frame.set(i, (state.closure)(i, frame.get(i)));
            }
            frame
        });

        ArrayProvider::<_, N>::new(crate::array_utils::collect_iter::<_, N>(res_iter))
    }
}

impl<T, F, ParSigCfg> IntoSignal for MapFrameSignalConfig<T, F, ParSigCfg>
where
    ParSigCfg: IntoSignal,
    MapFrameSignal<T, F, ParSigCfg::Signal>:
        Signal<State = MapFrameSignalState<IntoSignalState<ParSigCfg>, F>>,
{
    type Signal = MapFrameSignal<T, F, ParSigCfg::Signal>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let par = self.parent_config.into_signal()?;
        let signal = MapFrameSignal {
            parent: par.signal,
            _phantom: PD,
        };
        let state = MapFrameSignalState {
            closure: self.closure,
            par_state: par.state,
        };
        Ok(ReadySignal { signal, state })
    }

    fn trace<F2: FnMut(crate::unique_id::UniqueId, TracedResource)>(
        &mut self,
        inserter: &mut F2,
    ) -> crate::Result<()> {
        self.parent_config.trace(inserter)?;
        Ok(())
    }
}

impl<T, F, ParSigCfg> MapFrameSignalConfig<T, F, ParSigCfg> {
    pub fn new(parent_config: ParSigCfg, closure: F) -> Self {
        Self {
            parent_config,
            closure,
            _phantom: PD,
        }
    }
}

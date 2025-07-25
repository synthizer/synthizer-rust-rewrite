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
    F: FnMut(usize, &T) -> T + Send + Sync + 'static,
    ParSig: Signal,
    ParSig::Output: AudioFrame<T>,
    T: Copy + Default + 'static,
{
    type Input = ParSig::Input;
    type Output = ParSig::Output;
    type State = MapFrameSignalState<ParSig::State, F>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        ParSig::on_block_start(ctx, &mut state.par_state);
    }

    fn tick_frame(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let mut frame = ParSig::tick_frame(ctx, input, &mut state.par_state);
        for i in 0..frame.channel_count() {
            frame.set(i, (state.closure)(i, frame.get(i)));
        }
        frame
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

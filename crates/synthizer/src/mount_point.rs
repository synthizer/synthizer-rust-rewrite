use crate::core_traits::*;
use crate::synthesizer::SynthesizerState;
use std::sync::Arc;

pub(crate) struct MountPoint<S: Mountable>
where
    S::State: Send + Sync + 'static,
    S::Parameters: Send + Sync + 'static,
{
    state: S::State,
    signal: S,
    time: u64,
}

pub(crate) trait ErasedMountPoint: Send + Sync + 'static {
    fn run(&mut self, state: &Arc<SynthesizerState>);
}

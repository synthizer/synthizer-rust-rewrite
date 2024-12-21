use std::sync::Arc;

use crate::config;
use crate::context::*;
use crate::core_traits::*;
use crate::synthesizer::SynthesizerState;

use crate::unique_id::UniqueId;

pub(crate) struct MountPoint<S: Mountable>
where
    S::State: Send + Sync + 'static,
    S::Parameters: Send + Sync + 'static,
{
    pub(crate) signal: S,
    pub(crate) state: S::State,
}

pub(crate) trait ErasedMountPoint: Send + Sync + 'static {
    /// The id here lets the mount look up other things outside the mount.
    fn run(
        &mut self,
        state: &Arc<SynthesizerState>,
        mount_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    );
}

impl<S: Mountable> ErasedMountPoint for MountPoint<S> {
    fn run(
        &mut self,
        state: &Arc<SynthesizerState>,
        mount_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    ) {
        let sig_state = &mut self.state;
        let parameters = state
            .mounts
            .get(mount_id)
            .expect("We are in a mount that should be in this map")
            .parameters
            .downcast_ref::<S::Parameters>()
            .expect("These are parameters for this mount");
        let ctx = SignalExecutionContext { fixed: shared_ctx };

        S::on_block_start(&ctx, parameters, &mut *sig_state);
        S::tick::<_, { config::BLOCK_SIZE }>(
            &ctx,
            [(); config::BLOCK_SIZE],
            parameters,
            &mut *sig_state,
            |_: [(); config::BLOCK_SIZE]| {},
        );
    }
}

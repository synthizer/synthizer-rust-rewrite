#![allow(private_interfaces)]

use crate::config;
use crate::context::*;
use crate::core_traits::*;
use crate::error::Result;
use crate::unique_id::UniqueId;
use crate::Chain;

pub(crate) struct MountPoint<S: ExecutableMount>
where
    S::State: Send + Sync + 'static,
{
    pub(crate) handler: S,
    pub(crate) state: S::State,
}

pub(crate) trait ErasedMountPoint: Send + Sync + 'static {
    /// The id here lets the mount look up other things outside the mount.
    fn run(
        &mut self,
        mount_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    );
}

pub mod sealed {
    use super::*;

    pub trait ExecutableMount: Send + Sync + 'static + Sized {
        type State: Send + Sync + 'static;

        fn run(
            mount: &mut MountPoint<Self>,
            mount_id: &UniqueId,
            shared_ctx: &FixedSignalExecutionContext,
        );
    }

    pub trait Mountable {
        fn into_mount(
            self,
            batch: &mut crate::synthesizer::Batch,
        ) -> Result<Box<dyn ErasedMountPoint>>;
    }
}

pub(crate) use sealed::*;

impl<S> ExecutableMount for S
where
    S: Signal<Input = (), Output = ()>,
{
    type State = SignalState<S>;

    fn run(
        mount: &mut MountPoint<Self>,
        _mount_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    ) {
        let sig_state = &mut mount.state;

        let ctx = SignalExecutionContext { fixed: shared_ctx };

        S::on_block_start(&ctx, &mut *sig_state);

        // Process the block frame by frame
        for _ in 0..config::BLOCK_SIZE {
            S::tick_frame(&ctx, (), &mut *sig_state);
        }
    }
}

impl<S: ExecutableMount> ErasedMountPoint for MountPoint<S> {
    fn run(
        &mut self,
        mount_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    ) {
        S::run(self, mount_id, shared_ctx);
    }
}

impl<S: IntoSignal> Mountable for Chain<S>
where
    S::Signal: Signal<Input = (), Output = ()>,
{
    fn into_mount(
        self,
        _batch: &mut crate::synthesizer::Batch,
    ) -> Result<Box<dyn ErasedMountPoint>> {
        let ready = self.into_signal()?;

        let mp = MountPoint {
            handler: ready.signal,
            state: ready.state,
        };

        Ok(Box::new(mp))
    }
}

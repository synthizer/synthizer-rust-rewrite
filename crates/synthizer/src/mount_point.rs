#![allow(private_interfaces)]
use std::any::Any;
use std::sync::Arc;

use crate::config;
use crate::context::*;
use crate::core_traits::*;
use crate::error::Result;
use crate::synthesizer::SynthesizerState;
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
        state: &Arc<SynthesizerState>,
        mount_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    );

    fn trace_slots(&self, tracer: &mut dyn FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>));
}

pub mod sealed {
    use super::*;

    pub trait ExecutableMount: Send + Sync + 'static + Sized {
        type State: Send + Sync + 'static;

        fn trace_slots(
            state: &Self::State,
            inserter: &mut dyn FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>),
        );

        fn run(
            mount: &mut MountPoint<Self>,
            state: &Arc<SynthesizerState>,
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
    for<'il, 'ol> S: Signal<Input<'il> = (), Output<'ol> = ()>,
{
    type State = SignalState<S>;

    fn trace_slots(
        state: &Self::State,
        mut inserter: &mut dyn FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>),
    ) {
        Self::trace_slots(state, &mut inserter);
    }

    fn run(
        mount: &mut MountPoint<Self>,
        _state: &Arc<SynthesizerState>,
        _mount_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    ) {
        let sig_state = &mut mount.state;

        let ctx = SignalExecutionContext { fixed: shared_ctx };

        S::on_block_start(&ctx, &mut *sig_state);
        S::tick::<_, { config::BLOCK_SIZE }>(
            &ctx,
            ArrayProvider::new([(); config::BLOCK_SIZE]),
            &mut *sig_state,
        );
    }
}

impl<S: ExecutableMount> ErasedMountPoint for MountPoint<S> {
    fn run(
        &mut self,
        state: &Arc<SynthesizerState>,
        mount_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    ) {
        S::run(self, state, mount_id, shared_ctx);
    }

    fn trace_slots(
        &self,
        mut tracer: &mut dyn FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>),
    ) {
        <S as ExecutableMount>::trace_slots(&self.state, &mut tracer);
    }
}

impl<S: IntoSignal> Mountable for Chain<S>
where
    for<'il, 'ol> S::Signal: Signal<Input<'il> = (), Output<'ol> = ()>,
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

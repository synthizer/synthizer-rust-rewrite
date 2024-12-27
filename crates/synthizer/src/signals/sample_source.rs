use arrayvec::ArrayVec;

use crate::config;
use crate::core_traits::*;
use crate::sample_sources::{execution::Executor, Descriptor as SDescriptor, SampleSource};

pub(crate) struct SampleSourceSignalState {
    source: Executor,

    /// Intermediate buffer to let us convert to `f64`.
    ///
    /// Always `BLOCK_SIZE * channels`, and refilled on block start.
    buf: Vec<f32>,

    /// How far we have read in our buffer, in frames.
    ///
    /// Maximum is `BLOCK_SIZE`.F
    buf_read: usize,
}

pub(crate) struct SampleSourceSignal;

impl Signal for SampleSourceSignal {
    type Input<'il> = ();
    type Output<'ol> = ArrayVec<f64, config::MAX_CHANNELS>;
    type state = SampleSourceSignalState;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
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
    }

    fn trace_slots<
        F: FnMut(
            crate::unique_id::UniqueId,
            std::sync::Arc<dyn std::any::Any + Send + Sync + 'static>,
        ),
    >(
        _state: &Self::State,
        _inserter: &mut F,
    ) {
    }
}

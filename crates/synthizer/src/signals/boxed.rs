use std::any::Any;
use std::marker::PhantomData as PD;

use crate::context::*;
use crate::core_traits::*;
use crate::error::Result;
use crate::signals::FrameBatcher;
use crate::unique_id::UniqueId;

/// A signal behind a box, whose input is `I` and output is `O`.
///
/// This is almost as efficient as the non-boxed version, save in recursive structures.  In practice, this value is not
/// allocating itself, as signals are zero-sized.  The primary use of this is compile times and error messages: it can
/// be used to simplify the types to something readable and manageable, both for humans and the compiler.
///
/// Unfortunately, however, it is required that the input and output be `'static` and `Copy`.  This is due to the
/// inability to pass owned arrays of varying type to the underlying signal.
pub struct BoxedSignalConfig<I, O> {
    signal: Box<dyn ErasedIntoSignal<I, O>>,
}

pub struct BoxedSignal<I, O> {
    _phantom: PD<(I, O)>,
}

pub struct BoxedSignalState<I, O> {
    signal: Box<dyn ErasedSignal<I, O>>,
    state: Box<dyn Any + Send + Sync + 'static>,
    batcher: FrameBatcher<I, O>,
}

trait ErasedSignal<I, O>
where
    Self: 'static + Send + Sync,
    I: 'static + Copy,
    O: 'static + Copy,
{
    fn on_block_start_erased(&self, ctx: &SignalExecutionContext<'_, '_>, state: &mut dyn Any);

    /// Process multiple frames at once for efficiency
    fn tick_frames_erased(
        &self,
        ctx: &SignalExecutionContext<'_, '_>,
        inputs: &[I],
        outputs: &mut [O],
        state: &mut dyn Any,
    );
}

trait ErasedIntoSignal<I, O>
where
    Self: 'static + Send + Sync,
    I: 'static + Copy,
    O: 'static + Copy,
{
    #[allow(clippy::type_complexity)]
    fn erased_into(&mut self) -> Result<ReadySignal<BoxedSignal<I, O>, BoxedSignalState<I, O>>>;

    fn trace_erased(&mut self, tracer: &mut dyn FnMut(UniqueId, TracedResource)) -> Result<()>;
}

impl<T, I, O> ErasedIntoSignal<I, O> for Option<T>
where
    I: 'static + Copy,
    O: 'static + Copy + Default,
    T: IntoSignal + Send + Sync + 'static,
    T::Signal: Signal<Input = I, Output = O> + 'static,
{
    fn erased_into(&mut self) -> Result<ReadySignal<BoxedSignal<I, O>, BoxedSignalState<I, O>>> {
        let underlying = self
            .take()
            .expect("This should never be called twice; we are using `Option<T>` to do a move at runtime")
            .into_signal()?;
        Ok(ReadySignal {
            signal: BoxedSignal { _phantom: PD },
            state: BoxedSignalState {
                signal: Box::new(underlying.signal),
                state: Box::new(underlying.state),
                batcher: FrameBatcher::new(),
            },
        })
    }

    fn trace_erased(&mut self, mut tracer: &mut dyn FnMut(UniqueId, TracedResource)) -> Result<()> {
        self.as_mut()
            .expect("Should not trace after conversion into a signal")
            .trace(&mut tracer)?;
        Ok(())
    }
}

impl<I, O, T> ErasedSignal<I, O> for T
where
    T: Signal<Input = I, Output = O>,
    I: 'static + Copy,
    O: 'static + Copy,
{
    fn on_block_start_erased(&self, ctx: &SignalExecutionContext<'_, '_>, state: &mut dyn Any) {
        let state = state.downcast_mut::<T::State>().unwrap();
        T::on_block_start(ctx, state);
    }

    fn tick_frames_erased(
        &self,
        ctx: &SignalExecutionContext<'_, '_>,
        inputs: &[I],
        outputs: &mut [O],
        state: &mut dyn Any,
    ) {
        let state = state.downcast_mut::<T::State>().unwrap();

        // Process each frame
        for (input, output) in inputs.iter().zip(outputs.iter_mut()) {
            *output = T::tick_frame(ctx, *input, state);
        }
    }
}

unsafe impl<I, O> Signal for BoxedSignal<I, O>
where
    I: Copy + Send + Sync + 'static,
    O: Copy + Send + Sync + 'static + Default,
{
    type Input = I;
    type Output = O;
    type State = BoxedSignalState<I, O>;

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        state.signal.on_block_start_erased(ctx, &mut *state.state);
        state.batcher.reset();
    }

    fn tick_frame(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        state.batcher.process_frame(input, |inputs, outputs| {
            state
                .signal
                .tick_frames_erased(ctx, inputs, outputs, &mut *state.state);
        })
    }
}

impl<I, O> BoxedSignalConfig<I, O>
where
    I: Copy + 'static,
    O: Copy + 'static + Default,
{
    pub(crate) fn new<S>(underlying: S) -> Self
    where
        S: IntoSignal + Send + Sync + 'static,
        S::Signal: Signal<Input = I, Output = O>,
    {
        Self {
            signal: Box::new(Some(underlying)),
        }
    }
}

impl<I, O> IntoSignal for BoxedSignalConfig<I, O>
where
    I: Copy + Send + Sync + 'static,
    O: Copy + Send + Sync + 'static + Default,
{
    type Signal = BoxedSignal<I, O>;

    fn into_signal(mut self) -> IntoSignalResult<Self> {
        self.signal.erased_into()
    }

    fn trace<F: FnMut(UniqueId, TracedResource)>(&mut self, inserter: &mut F) -> Result<()> {
        self.signal.trace_erased(inserter)?;
        Ok(())
    }
}

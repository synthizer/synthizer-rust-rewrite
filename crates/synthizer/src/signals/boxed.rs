use std::any::Any;
use std::marker::PhantomData as PD;
use std::mem::MaybeUninit;
use std::sync::Arc;

use crate::context::*;
use crate::core_traits::*;
use crate::error::Result;
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

pub struct BoxedSignalState {
    state: Box<dyn Any + Send + Sync + 'static>,
}

pub struct BoxedSignalParams<I, O> {
    signal: Box<dyn ErasedSignal<I, O>>,
    underlying_params: Box<dyn Any + Send + Sync + 'static>,
}

trait ErasedSignal<I, O>
where
    Self: 'static + Send + Sync,
    I: 'static + Copy,
    O: 'static + Copy,
{
    fn on_block_start_erased(
        &self,
        ctx: &SignalExecutionContext<'_, '_>,
        params: &dyn Any,
        state: &mut dyn Any,
    );

    fn trace_slots_erased(
        &self,
        state: &dyn Any,
        params: &dyn Any,
        tracer: &mut dyn FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>),
    );

    fn tick_erased(
        &self,
        ctx: &SignalExecutionContext<'_, '_>,
        input: &[I],
        params: &dyn Any,
        state: &mut dyn Any,
        output: &mut dyn FnMut(O),
    );
}

trait ErasedIntoSignal<I, O>
where
    Self: 'static + Send + Sync,
    I: 'static + Copy,
    O: 'static + Copy,
{
    #[allow(clippy::type_complexity)]
    fn erased_into(
        &mut self,
    ) -> Result<ReadySignal<BoxedSignal<I, O>, BoxedSignalState, BoxedSignalParams<I, O>>>;
}

impl<T, I, O> ErasedIntoSignal<I, O> for Option<T>
where
    I: 'static + Copy,
    O: 'static + Copy,
    T: IntoSignal + Send + Sync + 'static,
    for<'il, 'ol> T::Signal: Signal<Input<'il> = I, Output<'ol> = O> + 'static,
{
    fn erased_into(
        &mut self,
    ) -> Result<ReadySignal<BoxedSignal<I, O>, BoxedSignalState, BoxedSignalParams<I, O>>> {
        let underlying = self
            .take()
            .expect("This should never be called twice; we are using `Option<T>` to do a move at runtime")
            .into_signal()?;
        Ok(ReadySignal {
            signal: BoxedSignal { _phantom: PD },
            parameters: BoxedSignalParams {
                signal: Box::new(underlying.signal),
                underlying_params: Box::new(underlying.parameters),
            },
            state: BoxedSignalState {
                state: Box::new(underlying.state),
            },
        })
    }
}

impl<I, O, T> ErasedSignal<I, O> for T
where
    for<'il, 'ol> T: Signal<Input<'il> = I, Output<'ol> = O>,
    I: 'static + Copy,
    O: 'static + Copy,
{
    fn trace_slots_erased(
        &self,
        state: &dyn Any,
        params: &dyn Any,
        mut tracer: &mut dyn FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>),
    ) {
        let params = params.downcast_ref::<BoxedSignalParams<I, O>>().unwrap();
        let state = state.downcast_ref::<BoxedSignalState>().unwrap();
        let underlying_params = params
            .underlying_params
            .downcast_ref::<T::Parameters>()
            .unwrap();
        let underlying_state = state.state.downcast_ref::<T::State>().unwrap();
        T::trace_slots(underlying_state, underlying_params, &mut tracer);
    }

    fn on_block_start_erased(
        &self,
        ctx: &SignalExecutionContext<'_, '_>,
        params: &dyn Any,
        state: &mut dyn Any,
    ) {
        let params = params.downcast_ref::<BoxedSignalParams<I, O>>().unwrap();
        let state = state.downcast_mut::<BoxedSignalState>().unwrap();
        let underlying_params = params
            .underlying_params
            .downcast_ref::<T::Parameters>()
            .unwrap();
        let underlying_state = state.state.downcast_mut::<T::State>().unwrap();
        T::on_block_start(ctx, underlying_params, underlying_state);
    }

    fn tick_erased(
        &self,
        ctx: &SignalExecutionContext<'_, '_>,
        mut input: &[I],
        params: &dyn Any,
        state: &mut dyn Any,
        mut output: &mut dyn FnMut(O),
    ) {
        let params = params.downcast_ref::<BoxedSignalParams<I, O>>().unwrap();
        let state = state.downcast_mut::<BoxedSignalState>().unwrap();
        let underlying_params = params
            .underlying_params
            .downcast_ref::<T::Parameters>()
            .unwrap();
        let underlying_state = state.state.downcast_mut::<T::State>().unwrap();

        macro_rules! do_one {
            ($num: expr) => {
                while let Some(this_input) = input.first_chunk::<$num>().copied() {
                    T::tick::<_, $num>(
                        ctx,
                        this_input,
                        underlying_params,
                        underlying_state,
                        |x: [O; $num]| {
                            x.into_iter().for_each(&mut output);
                        },
                    );
                    input = &input[$num..];
                }
            };
        }

        do_one!(8);
        do_one!(4);
        do_one!(1);
    }
}
// Rust is not happy about casting the references we have into `&dyn Any` without a reborrow.  It's kind of unclear why:
// the references we get have longer lifetimes than where they're going.  I'm guessing this is just a type inference
// weakness.
#[allow(clippy::borrow_deref_ref)]
unsafe impl<I, O> Signal for BoxedSignal<I, O>
where
    I: Copy + Send + Sync + 'static,
    O: Copy + Send + Sync + 'static,
{
    type Input<'il> = I;
    type Output<'ol> = O;
    type Parameters = BoxedSignalParams<I, O>;
    type State = BoxedSignalState;

    fn on_block_start(
        ctx: &SignalExecutionContext<'_, '_>,
        params: &Self::Parameters,
        state: &mut Self::State,
    ) {
        params
            .signal
            .on_block_start_erased(ctx, &*params, &mut *state);
    }

    fn tick<'il, 'ol, D, const N: usize>(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: [Self::Input<'il>; N],
        params: &Self::Parameters,
        state: &mut Self::State,
        destination: D,
    ) where
        D: SignalDestination<Self::Output<'ol>, N>,
        Self::Input<'il>: 'ol,
        'il: 'ol,
    {
        let mut dest: [MaybeUninit<O>; N] = [const { MaybeUninit::uninit() }; N];
        let mut i = 0;

        params
            .signal
            .tick_erased(ctx, &input, &*params, &mut *state, &mut |o| {
                dest[i].write(o);
                i += 1;
            });

        assert_eq!(i, N);

        unsafe { destination.send(dest.map(|x| x.assume_init())) };
    }

    fn trace_slots<F: FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>)>(
        state: &Self::State,
        parameters: &Self::Parameters,
        inserter: &mut F,
    ) {
        parameters
            .signal
            .trace_slots_erased(&*state, &*parameters, inserter);
    }
}

impl<I, O> BoxedSignalConfig<I, O>
where
    I: Copy + 'static,
    O: Copy + 'static,
{
    pub(crate) fn new<S>(underlying: S) -> Self
    where
        S: IntoSignal + Send + Sync + 'static,
        for<'il, 'ol> S::Signal: Signal<Input<'il> = I, Output<'ol> = O>,
    {
        Self {
            signal: Box::new(Some(underlying)),
        }
    }
}

impl<I, O> IntoSignal for BoxedSignalConfig<I, O>
where
    I: Copy + Send + Sync + 'static,
    O: Copy + Send + Sync + 'static,
{
    type Signal = BoxedSignal<I, O>;

    fn into_signal(
        mut self,
    ) -> Result<ReadySignal<Self::Signal, IntoSignalState<Self>, IntoSignalParameters<Self>>> {
        self.signal.erased_into()
    }
}

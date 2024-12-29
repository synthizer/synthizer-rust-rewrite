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

pub struct BoxedSignalState<I, O> {
    signal: Box<dyn ErasedSignal<I, O>>,
    state: Box<dyn Any + Send + Sync + 'static>,
}

trait ErasedSignal<I, O>
where
    Self: 'static + Send + Sync,
    I: 'static + Copy,
    O: 'static + Copy,
{
    fn on_block_start_erased(&self, ctx: &SignalExecutionContext<'_, '_>, state: &mut dyn Any);

    fn trace_slots_erased(
        &self,
        state: &dyn Any,
        tracer: &mut dyn FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>),
    );

    fn tick_erased(
        &self,
        ctx: &SignalExecutionContext<'_, '_>,
        input: &[I],
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
    fn erased_into(&mut self) -> Result<ReadySignal<BoxedSignal<I, O>, BoxedSignalState<I, O>>>;
}

impl<T, I, O> ErasedIntoSignal<I, O> for Option<T>
where
    I: 'static + Copy,
    O: 'static + Copy,
    T: IntoSignal + Send + Sync + 'static,
    for<'il, 'ol> T::Signal: Signal<Input<'il> = I, Output<'ol> = O> + 'static,
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
        mut tracer: &mut dyn FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>),
    ) {
        let state = state.downcast_ref::<T::State>().unwrap();
        T::trace_slots(state, &mut tracer);
    }

    fn on_block_start_erased(&self, ctx: &SignalExecutionContext<'_, '_>, state: &mut dyn Any) {
        let state = state.downcast_mut::<T::State>().unwrap();
        T::on_block_start(ctx, state);
    }

    fn tick_erased(
        &self,
        ctx: &SignalExecutionContext<'_, '_>,
        mut input: &[I],
        state: &mut dyn Any,
        mut output: &mut dyn FnMut(O),
    ) {
        let state = state.downcast_mut::<T::State>().unwrap();

        macro_rules! do_one {
            ($num: expr) => {
                while let Some(this_input) = input.first_chunk::<$num>().copied() {
                    let prov = T::tick::<_, $num>(ctx, ArrayProvider::new(this_input), state);
                    prov.iter_cloned().for_each(&mut output);
                    input = &input[$num..];
                }
            };
        }

        do_one!(8);
        do_one!(4);
        do_one!(1);
    }
}

unsafe impl<I, O> Signal for BoxedSignal<I, O>
where
    I: Copy + Send + Sync + 'static,
    O: Copy + Send + Sync + 'static,
{
    type Input<'il> = I;
    type Output<'ol> = O;
    type State = BoxedSignalState<I, O>;

    fn on_block_start(ctx: &SignalExecutionContext<'_, '_>, state: &mut Self::State) {
        state.signal.on_block_start_erased(ctx, &mut *state.state);
    }

    fn tick<'il, 'ol, IProvider, const N: usize>(
        ctx: &'_ SignalExecutionContext<'_, '_>,
        input: IProvider,
        state: &mut Self::State,
    ) -> impl ValueProvider<Self::Output<'ol>>
    where
        Self::Input<'il>: 'ol,
        'il: 'ol,
        IProvider: ValueProvider<Self::Input<'il>> + Sized,
    {
        let mut dest: [MaybeUninit<O>; N] = [const { MaybeUninit::uninit() }; N];
        let mut i = 0;

        let in_arr = crate::array_utils::collect_iter::<_, N>(input.iter_cloned());

        state
            .signal
            .tick_erased(ctx, &in_arr, &mut *state.state, &mut |o| {
                dest[i].write(o);
                i += 1;
            });

        assert_eq!(i, N);

        unsafe { ArrayProvider::<_, N>::new(dest.map(|x| x.assume_init())) }
    }

    fn trace_slots<F: FnMut(UniqueId, Arc<dyn Any + Send + Sync + 'static>)>(
        state: &Self::State,
        inserter: &mut F,
    ) {
        state.signal.trace_slots_erased(&*state.state, inserter);
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

    fn into_signal(mut self) -> IntoSignalResult<Self> {
        self.signal.erased_into()
    }
}

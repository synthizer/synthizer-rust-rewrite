use std::cell::RefCell;
use std::marker::PhantomData as PD;
use std::num::NonZeroUsize;
use std::sync::Arc;

use crate::core_traits::*;
use crate::data_structures::*;

type DelayLineStatePtr<T> = Arc<ExclusiveThreadCell<RefCell<DelayLineState<T>>>>;

/// A handle to a delay line.
///
/// Delay lines are intended to be used in only one chain at  a time.  If used in multiple chains, the promise is merely
/// that there will not be a crash.  The actual result will be that the data in the line is randomly filled with data
/// from other places.  In general, you want to define a place where the line is written, and a second place where the
/// line is read from.
///
/// There are two ways to use a line.
///
/// The first is to use [DelayLine::read] and [DelayLine::write] separately.  In this case, the order of the execution
/// is the order of the chain.  In general such usage implies that reading will always be one sample behind.  If reading
/// is always one sample behind the execution order doesn't matter, as the current sample is always written and can be
/// read at soonest on the next sample.
///
/// The other way to use this is [DelayLine::write_read], which will write the line and immediately read it.  This
/// allows for delays of 0 to return the current value.
///
/// The functions for creating lines take a usize to represent the length of the line.  This is generally `secs *
/// sample_rate` where `secs` is the duration and `sample_rate` the sample rate of the library.  Consider
/// [crate::Synthesizer::duration_to_samples] to get from [std::time::Duration] to a line long enough to fulfill at
/// least that much time of data, or the same method on [crate::Batch] if you are already borrowing the synthesizer.
///
/// Positive delays are into the past.  Going past the end of the line wraps to avoid expensive runtime checks and/or
/// unsafe code.
///
/// If performing recursion with frames of addable types consider [DelayLineHandle::read_adding], to add instead of
/// replace.  If performing recursion with frames specifically of f64 values, consider
/// [DelayLineHandle::read_linear_interp2] to read the line with linear interpolation between the two nearest samples.
pub struct DelayLineHandle<T> {
    pub(crate) inner: DelayLineStatePtr<T>,
}

pub(crate) struct DelayLineState<T> {
    data: Vec<T>,
}

/// Signal to perform a read of a delay line, no interpolation.
pub(crate) struct DelayLineReadSignal<T, S> {
    _position_sig: S,
    _phantom: PD<T>,
}

pub(crate) struct DelayLineSignalState<T, ParState, MergeClosure> {
    line: DelayLineStatePtr<T>,
    par_sig_state: ParState,
    merger: MergeClosure,
    offset: usize,
}

unsafe impl<T, S> Send for DelayLineReadSignal<T, S> {}
unsafe impl<T, S> Sync for DelayLineReadSignal<T, S> {}

unsafe impl<T, S> Signal for DelayLineReadSignal<T, S>
where
    S: Signal<Output = usize>,
    T: Clone + Send + 'static,
{
    type Input = S::Input;
    type Output = T;
    type State = DelayLineSignalState<T, S::State, ()>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        S::on_block_start(ctx, &mut state.par_sig_state);
    }

    fn tick_frame(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let delay = S::tick_frame(ctx, input, &mut state.par_sig_state);
        let line = state.line.borrow();
        let line = line.borrow();
        let delay_samples = delay % line.data.len();
        // `delay_samples < len`, so `len - delay_samples > 0`.  By adding an extra length we avoid underflow.
        let index = (line.data.len() + state.offset - delay_samples) % line.data.len();

        let val = line.data[index].clone();
        state.offset = (state.offset + 1) % line.data.len();

        val
    }
}

pub(crate) struct DelayLineWriteSignal<T, S, M> {
    _parent_signal: S,
    _phantom: PD<(T, M)>,
}

unsafe impl<T, S, M> Send for DelayLineWriteSignal<T, S, M> {}
unsafe impl<T, S, M> Sync for DelayLineWriteSignal<T, S, M> {}

unsafe impl<T, S, M> Signal for DelayLineWriteSignal<T, S, M>
where
    T: Send + 'static,
    S: Signal<Output = T>,
    M: FnMut(&mut T, &T) + Send + Sync + 'static,
{
    type Input = S::Input;
    type Output = ();
    type State = DelayLineSignalState<T, S::State, M>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        S::on_block_start(ctx, &mut state.par_sig_state);
    }

    fn tick_frame(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let value = S::tick_frame(ctx, input, &mut state.par_sig_state);
        let line = state.line.borrow();
        let mut line = line.borrow_mut();

        (state.merger)(&mut line.data[state.offset], &value);
        state.offset = (state.offset + 1) % line.data.len();
    }
}

// It is actually worth merging reading and writing by hand ourselves.  This avoids huge types and also provides some
// guaranteed performance optimization.  The user gets to the combined signal via [Chain::join].
struct DelayLineRwSignal<T, S, M> {
    par_sig: S,
    _phantom: PD<(T, M)>,
}

unsafe impl<T, S, M> Send for DelayLineRwSignal<T, S, M> {}
unsafe impl<T, S, M> Sync for DelayLineRwSignal<T, S, M> {}
unsafe impl<T, S, M> Signal for DelayLineRwSignal<T, S, M>
where
    S: Signal<Output = (usize, T)>,
    T: Clone + Send + 'static,
    M: FnMut(&mut T, &T) + Send + Sync + 'static,
{
    type Input = S::Input;
    type Output = T;
    type State = DelayLineSignalState<T, S::State, M>;

    fn on_block_start(
        ctx: &crate::context::SignalExecutionContext<'_, '_>,
        state: &mut Self::State,
    ) {
        S::on_block_start(ctx, &mut state.par_sig_state);
    }

    fn tick_frame(
        ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
        input: Self::Input,
        state: &mut Self::State,
    ) -> Self::Output {
        let (delay, value) = S::tick_frame(ctx, input, &mut state.par_sig_state);
        let line = state.line.borrow();
        let mut line = line.borrow_mut();

        let write_ind = state.offset % line.data.len();
        let delay_samples = delay % line.data.len();
        let read_ind = (line.data.len() + state.offset - delay_samples) % line.data.len();
        let read_val = line.data[read_ind].clone();

        (state.merger)(&mut line.data[write_ind], &value);
        state.offset = (state.offset + 1) % line.data.len();

        read_val
    }
}

pub(crate) struct DelayLineReadSignalConfig<T, S> {
    pub(crate) parent: S,
    pub(crate) line: DelayLineStatePtr<T>,
}

impl<T, S> IntoSignal for DelayLineReadSignalConfig<T, S>
where
    DelayLineReadSignal<T, S::Signal>:
        Signal<State = DelayLineSignalState<T, IntoSignalState<S>, ()>>,
    S: IntoSignal,
{
    type Signal = DelayLineReadSignal<T, S::Signal>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let parent = self.parent.into_signal()?;
        Ok(ReadySignal {
            state: DelayLineSignalState::<T, IntoSignalState<S>, ()> {
                line: self.line,
                par_sig_state: parent.state,
                merger: (),
                offset: 0,
            },
            signal: DelayLineReadSignal::<T, S::Signal> {
                _position_sig: parent.signal,
                _phantom: PD,
            },
        })
    }
}

pub(crate) struct DelayLineWriteSignalConfig<T, S, M> {
    pub(crate) parent: S,
    pub(crate) line: DelayLineStatePtr<T>,
    pub(crate) merger: M,
}

impl<T, S, M> IntoSignal for DelayLineWriteSignalConfig<T, S, M>
where
    DelayLineWriteSignal<T, S::Signal, M>:
        Signal<State = DelayLineSignalState<T, IntoSignalState<S>, M>>,
    S: IntoSignal,
{
    type Signal = DelayLineWriteSignal<T, S::Signal, M>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let parent = self.parent.into_signal()?;
        Ok(ReadySignal {
            state: DelayLineSignalState::<T, IntoSignalState<S>, M> {
                line: self.line,
                par_sig_state: parent.state,
                merger: self.merger,
                offset: 0,
            },
            signal: DelayLineWriteSignal::<T, S::Signal, M> {
                _parent_signal: parent.signal,
                _phantom: PD,
            },
        })
    }
}

struct DelayLineRwSignalConfig<T, S, M> {
    parent: S,
    line: DelayLineStatePtr<T>,
    merger: M,
}

impl<T, S, M> IntoSignal for DelayLineRwSignalConfig<T, S, M>
where
    DelayLineRwSignal<T, S::Signal, M>:
        Signal<State = DelayLineSignalState<T, IntoSignalState<S>, M>>,
    S: IntoSignal,
{
    type Signal = DelayLineRwSignal<T, S::Signal, M>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let parent = self.parent.into_signal()?;
        Ok(ReadySignal {
            state: DelayLineSignalState::<T, IntoSignalState<S>, M> {
                line: self.line,
                par_sig_state: parent.state,
                merger: self.merger,
                offset: 0,
            },
            signal: DelayLineRwSignal::<T, S::Signal, M> {
                par_sig: parent.signal,
                _phantom: PD,
            },
        })
    }
}

impl<T> DelayLineHandle<T> {
    /// Create a delay line given a length and a factory function.
    pub fn new<F>(length: NonZeroUsize, mut factory: F) -> Self
    where
        F: FnMut() -> T,
    {
        let length = length.get();
        let data = (0..length).map(|_| factory()).collect::<Vec<_>>();
        Self {
            inner: Arc::new(ExclusiveThreadCell::new(RefCell::new(DelayLineState {
                data,
            }))),
        }
    }

    pub fn new_defaulting(length: NonZeroUsize) -> Self
    where
        T: Default,
    {
        Self::new(length, Default::default)
    }

    pub fn new_cloningf(length: NonZeroUsize, value: T) -> Self
    where
        T: Clone,
    {
        Self::new(length, || value.clone())
    }
}

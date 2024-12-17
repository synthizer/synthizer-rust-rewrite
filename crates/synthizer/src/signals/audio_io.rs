use crate::config;
use crate::context::*;
use crate::core_traits::*;

pub struct AudioOutputSignal<S>(S);
pub struct AudioOutputSignalConfig<S>(S);

unsafe impl<S> Signal for AudioOutputSignal<S>
where
    S: Signal<Output = f64>,
{
    type Output = ();
    type Input = S::Input;
    // The parent state, with a usize tacked on for tick1's counter.
    type State = (S::State, usize);
    type Parameters = S::Parameters;

    fn tick1<D: SignalDestination<Self::Output>>(
        ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        input: &'_ Self::Input,
        destination: D,
    ) {
        let mut val: Option<S::Output> = None;
        S::tick1(&mut ctx.wrap(|s| &mut s.0, |p| p), input, |x| val = Some(x));

        // We output the unit type instead.
        destination.send(());

        // Later this will go to a bus. But we are not at buses yet.
        ctx.fixed.audio_destinationh[ctx.state.1] = val.unwrap();
        ctx.state.1 += 1;
    }

    fn on_block_start(ctx: &mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>) {
        S::on_block_start(&mut ctx.wrap(|s| &mut s.0, |p| p));

        // If the caller decides to use tick1, this index will be incremented. Reset it.
        ctx.state.1 = 0;
    }

    fn tick_block<
        'a,
        I: FnMut(usize) -> &'a Self::Input,
        D: ReusableSignalDestination<Self::Output>,
    >(
        ctx: &'_ mut SignalExecutionContext<'_, '_, Self::State, Self::Parameters>,
        input: I,
        mut destination: D,
    ) where
        Self::Input: 'a,
    {
        let mut block: [f64; config::BLOCK_SIZE] = [0.0f64; config::BLOCK_SIZE];

        let mut i = 0;
        S::tick_block(&mut ctx.wrap(|s| &mut s.0, |p| p), input, |x| {
            block[i] = x;
            i += 1;
        });

        // This is a good place for an assert because it is a final output; if any parent signal did not call the
        // destination exactly once per sample, we'll notice.
        debug_assert_eq!(i, config::BLOCK_SIZE);

        ctx.fixed.audio_destinationh.copy_from_slice(&block);

        // We do have to actually use the destination, as this drives computations elsewhere.
        for _ in 0..config::BLOCK_SIZE {
            destination.send_reusable(());
        }
    }
}

impl<S> IntoSignal for AudioOutputSignalConfig<S>
where
    S: IntoSignal,
    S::Signal: Signal<Output = f64>,
{
    type Signal = AudioOutputSignal<S::Signal>;

    fn into_signal(self) -> IntoSignalResult<Self> {
        let inner = self.0.into_signal()?;
        Ok(ReadySignal {
            signal: AudioOutputSignal(inner.signal),
            state: (inner.state, 0),
            parameters: inner.parameters,
        })
    }
}

impl<S> AudioOutputSignalConfig<S> {
    pub(crate) fn new(signal: S) -> Self {
        Self(signal)
    }
}

use std::marker::PhantomData;
use std::sync::Arc;

use crate::core_traits::*;
use crate::wavetable::WaveTable;

/// State for all wavetable signals
pub struct WaveTableSignalState<F> {
    pub(crate) wavetable: Arc<WaveTable>,
    pub(crate) position: f64,
    pub(crate) increment: f64,
    pub(crate) _phantom: PhantomData<F>,
}

/// Macro to generate wavetable reading signals with different interpolation methods
macro_rules! wavetable_signal {
    ($name:ident, $config_name:ident, $method:ident, $doc:literal) => {
        #[doc = $doc]
        pub struct $config_name<F, const LOOPING: bool> {
            pub(crate) wavetable: Arc<WaveTable>,
            pub(crate) increment: f64,
            pub(crate) _phantom: PhantomData<F>,
        }

        pub struct $name<F, const LOOPING: bool> {
            _phantom: PhantomData<F>,
        }

        unsafe impl<F, const LOOPING: bool> Signal for $name<F, LOOPING>
        where
            F: AudioFrame<f64> + Send + Sync + 'static,
        {
            type Input = (); // No input needed
            type Output = F;
            type State = WaveTableSignalState<F>;

            fn on_block_start(
                _ctx: &crate::context::SignalExecutionContext<'_, '_>,
                _state: &mut Self::State,
            ) {
                // Nothing to do
            }

            fn tick_frame(
                _ctx: &'_ crate::context::SignalExecutionContext<'_, '_>,
                _input: Self::Input,
                state: &mut Self::State,
            ) -> Self::Output {
                let output = state.wavetable.$method::<F, LOOPING>(state.position);
                state.position += state.increment;
                if LOOPING {
                    state.position = state
                        .position
                        .rem_euclid(state.wavetable.frame_count() as f64);
                }
                output
            }
        }

        impl<F, const LOOPING: bool> IntoSignal for $config_name<F, LOOPING>
        where
            F: AudioFrame<f64> + Send + Sync + 'static,
        {
            type Signal = $name<F, LOOPING>;

            fn into_signal(self) -> IntoSignalResult<Self> {
                Ok(ReadySignal {
                    signal: $name {
                        _phantom: PhantomData,
                    },
                    state: WaveTableSignalState {
                        wavetable: self.wavetable,
                        position: 0.0,
                        increment: self.increment,
                        _phantom: PhantomData,
                    },
                })
            }
        }
    };
}

// Generate the three signal types
wavetable_signal!(
    WaveTableTruncatedSignal,
    WaveTableTruncatedSignalConfig,
    read_truncated,
    "Signal that reads from a wavetable using truncation (no interpolation)"
);

wavetable_signal!(
    WaveTableLinearSignal,
    WaveTableLinearSignalConfig,
    read_linear,
    "Signal that reads from a wavetable using linear interpolation"
);

wavetable_signal!(
    WaveTableCubicSignal,
    WaveTableCubicSignalConfig,
    read_cubic,
    "Signal that reads from a wavetable using cubic interpolation"
);

use std::sync::Arc;

use crate::config;
use crate::context::{FixedSignalExecutionContext, SignalExecutionContext};
use crate::core_traits::{IntoSignal, Signal, SignalState};
use crate::error::Result;
use crate::synthesizer::SynthesizerState;
use crate::unique_id::UniqueId;
use crate::Chain;

/// A program is a collection of signal fragments that execute together as a unit.
///
/// Programs are the primary unit of execution in the synthesizer. Each program can contain multiple fragments, where
/// each fragment processes audio independently. All fragments in a program share the same execution context and run
/// sequentially.
#[derive(Default)]
pub struct Program {
    fragments: Vec<Box<dyn ProgramFragment>>,
}

impl Program {
    /// Create a new empty program.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a signal fragment to this program.
    ///
    /// The signal must have no input and no output (Input = (), Output = ()).
    /// This is because fragments run independently and don't pass data between each other.
    pub fn add_fragment<S>(&mut self, signal_config: S) -> Result<()>
    where
        S: IntoSignal,
        S::Signal: Signal<Input = (), Output = ()> + 'static,
    {
        let ready = signal_config.into_signal()?;

        let fragment = SignalFragment {
            signal: ready.signal,
            state: ready.state,
        };

        self.fragments.push(Box::new(fragment));
        Ok(())
    }

    /// Execute one block of audio for all fragments in this program.
    pub(crate) fn execute_block(
        &mut self,
        state: &Arc<SynthesizerState>,
        program_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    ) {
        for fragment in &mut self.fragments {
            fragment.execute_block(state, program_id, shared_ctx);
        }
    }
}

/// Trait for program fragments that can be executed as part of a program.
///
/// This is the type-erased interface that allows different types of fragments to be stored and executed together.
pub(crate) trait ProgramFragment: Send + Sync + 'static {
    /// Execute one block of audio processing for this fragment.
    fn execute_block(
        &mut self,
        state: &Arc<SynthesizerState>,
        program_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    );
}

/// A program fragment that wraps a signal and its state.
///
/// This is the concrete implementation that allows signals to be used as fragments.
struct SignalFragment<S>
where
    S: Signal<Input = (), Output = ()>,
{
    signal: S,
    state: SignalState<S>,
}

impl<S> ProgramFragment for SignalFragment<S>
where
    S: Signal<Input = (), Output = ()>,
{
    fn execute_block(
        &mut self,
        _state: &Arc<SynthesizerState>,
        _program_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    ) {
        let ctx = SignalExecutionContext { fixed: shared_ctx };

        // Call on_block_start once per block
        S::on_block_start(&ctx, &mut self.state);

        // Process each frame in the block
        for _ in 0..config::BLOCK_SIZE {
            S::tick_frame(&ctx, (), &mut self.state);
        }
    }
}

/// Convert a Chain into a single-fragment Program.
///
/// This cannot fail for valid chains, so we panic if add_fragment fails. In practice, add_fragment only fails if the
/// signal's into_signal() fails, which should not happen for properly constructed chains.
impl<S> From<Chain<S>> for Program
where
    S: IntoSignal,
    S::Signal: Signal<Input = (), Output = ()> + 'static,
{
    fn from(chain: Chain<S>) -> Self {
        let mut program = Program::new();
        program
            .add_fragment(chain)
            .expect("Failed to convert chain to program - signal initialization failed");
        program
    }
}

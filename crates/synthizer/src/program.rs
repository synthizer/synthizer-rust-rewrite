
use crate::config;
use crate::context::{FixedSignalExecutionContext, SignalExecutionContext};
use crate::core_traits::{IntoSignal, Signal, SignalState};
use crate::error::Result;
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
    /// The signal must have no input (Input = ()) but can have any output type.
    /// The output is discarded as fragments run independently.
    pub fn add_fragment<S, O>(&mut self, signal_config: S) -> Result<()>
    where
        S: IntoSignal,
        S::Signal: Signal<Input = (), Output = O> + 'static,
        O: 'static,
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
        program_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    ) {
        for fragment in &mut self.fragments {
            fragment.execute_block(program_id, shared_ctx);
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
        program_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    );
}

/// A program fragment that wraps a signal and its state.
///
/// This is the concrete implementation that allows signals to be used as fragments.
struct SignalFragment<S>
where
    S: Signal<Input = ()>,
{
    signal: S,
    state: SignalState<S>,
}

impl<S> ProgramFragment for SignalFragment<S>
where
    S: Signal<Input = ()>,
{
    fn execute_block(
        &mut self,
        _program_id: &UniqueId,
        shared_ctx: &FixedSignalExecutionContext,
    ) {
        let ctx = SignalExecutionContext { fixed: shared_ctx };

        // Call on_block_start once per block
        S::on_block_start(&ctx, &mut self.state);

        // Process each frame in the block
        for _ in 0..config::BLOCK_SIZE {
            // Discard the output
            let _ = S::tick_frame(&ctx, (), &mut self.state);
        }
    }
}

/// Convert a Chain into a single-fragment Program.
impl<S> TryFrom<Chain<S>> for Program
where
    S: IntoSignal,
    S::Signal: Signal<Input = (), Output = ()> + 'static,
{
    type Error = crate::error::Error;
    
    fn try_from(chain: Chain<S>) -> Result<Self> {
        let mut program = Program::new();
        program.add_fragment(chain)?;
        Ok(program)
    }
}

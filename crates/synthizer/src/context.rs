use crate::config;

pub struct SignalExecutionContext<'a, 'shared, TState, TParameters> {
    pub(crate) state: &'a mut TState,
    pub(crate) parameters: &'a TParameters,

    // The index we are currently in, in the current block. Combined with the shared time, this can be used to put a
    // timestamp back together.
    pub(crate) subblock_index: usize,

    pub(crate) fixed: &'a mut FixedSignalExecutionContext<'shared>,
}

/// Parts of the execution context which do not contain references that need to be recast.
pub(crate) struct FixedSignalExecutionContext<'a> {
    pub(crate) time_in_blocks: u64,
    pub(crate) audio_destinationh: &'a mut [f64; config::BLOCK_SIZE],
}

impl<'shared, TState, TParameters> SignalExecutionContext<'_, 'shared, TState, TParameters> {
    /// Convert this context into values usually derived from reborrows of this context's fields.  Used to grab parts of
    /// contexts when moving upstream.
    pub(crate) fn wrap<'a, NewS, NewP>(
        &'a mut self,
        new_state: impl FnOnce(&'a mut TState) -> &'a mut NewS,
        new_params: impl FnOnce(&'a TParameters) -> &'a NewP,
    ) -> SignalExecutionContext<'a, 'shared, NewS, NewP>
    where
        'shared: 'a,
    {
        SignalExecutionContext {
            state: new_state(self.state),
            parameters: new_params(self.parameters),
            fixed: self.fixed,
            subblock_index: self.subblock_index,
        }
    }
}

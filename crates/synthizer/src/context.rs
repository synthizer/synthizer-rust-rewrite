use atomic_refcell::AtomicRefCell;

use crate::config;
use crate::signals::SlotUpdateContext;

pub struct SignalExecutionContext<'a, 'shared> {
    pub(crate) fixed: &'a FixedSignalExecutionContext<'shared>,
}

/// Parts of the execution context which do not contain references that need to be recast.
pub(crate) struct FixedSignalExecutionContext<'a> {
    pub(crate) time_in_blocks: u64,
    pub(crate) audio_destinationh: AtomicRefCell<&'a mut [f64; config::BLOCK_SIZE]>,
    pub(crate) slots: &'a SlotUpdateContext<'a>,
}

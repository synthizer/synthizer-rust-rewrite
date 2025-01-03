use atomic_refcell::AtomicRefCell;

use crate::channel_format::*;
use crate::config;
use crate::signals::{MediaExecutorMap, SlotUpdateContext};

pub struct SignalExecutionContext<'a, 'shared> {
    pub(crate) fixed: &'a FixedSignalExecutionContext<'shared>,
}

/// Parts of the execution context which do not contain references that need to be recast.
pub(crate) struct FixedSignalExecutionContext<'a> {
    pub(crate) time_in_blocks: u64,
    pub(crate) audio_destinationh: AtomicRefCell<&'a mut [[f64; 2]; config::BLOCK_SIZE]>,
    pub(crate) audio_destination_format: &'a ChannelFormat,
    pub(crate) slots: &'a SlotUpdateContext<'a>,
    pub(crate) media: &'a MediaExecutorMap,
}

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::mark_dropped::MarkDropped;
use crate::unique_id::UniqueId;

/// State associated with a handle, tracking which slots it uses.
pub(crate) struct HandleState {
    /// Map from slot ID to its MarkDropped instance.
    /// When the handle drops, all these slots will be marked for deletion.
    pub(crate) slots: HashMap<UniqueId, Arc<MarkDropped>>,
}

impl HandleState {
    pub(crate) fn new() -> Self {
        Self {
            slots: HashMap::new(),
        }
    }
}

/// A handle which may be used to manipulate some object.
///
/// Handles keep objects alive.  When the last handle drops, the object does as well.
pub struct Handle {
    pub(crate) object_id: UniqueId,
    pub(crate) mark_drop: Arc<MarkDropped>,
    pub(crate) state: Arc<Mutex<HandleState>>,
}

impl Clone for Handle {
    fn clone(&self) -> Self {
        Self {
            object_id: self.object_id,
            mark_drop: self.mark_drop.clone(),
            state: self.state.clone(),
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // When a handle drops, mark all its associated slots for deletion
        let _state = self.state.lock().expect("Handle mutex poisoned");
        // The slots will be marked for deletion when their MarkDropped instances drop
        // This happens automatically when the HashMap is cleared

        // The handle's own mark_drop will be triggered when the Arc drops
    }
}

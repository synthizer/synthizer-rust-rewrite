use std::marker::PhantomData as PD;

use crate::unique_id::UniqueId;

/// Track changes to the inner value.
///
/// This can be placed in immutable data structures.  Whenever `replace` is called the inner value is replaced, and a
/// counter is incremented.  Opaque `ChangeTrackerToken`s can then be used by consumers to ask if a change has occurred.
///
/// The `Clone` impl here allows for placing these in immutable data structures.  It should never be the case that more
/// than one tracker exists, in the sense that there should always clearly be some "newest" tracker, but to play nice
/// with our RCU-style concurrency Clone is required.
#[derive(Clone, Debug, Default)]
pub(crate) struct ChangeTracker<T> {
    tracker_id: UniqueId,
    change_counter: u64,
    inner: T,
}

/// An opaque token from a change tracker, which can be used to ask if a value has changed.
///
/// The association with the tracker is weak.  If there is a mismatch between a tracker and its token, then a runtime
/// panic results.  Associations are made on first use of a token with a tracker, e.g. `Default::default()` does not
/// associate.
///
/// The default value of a token will see the tracker as changed.
#[derive(Clone, Debug)]
pub(crate) struct ChangeTrackerToken<T> {
    tracker_id: Option<UniqueId>,
    change_counter: Option<u64>,
    _phantom: PD<T>,
}

impl<T> ChangeTracker<T> {
    pub fn new() -> Self
    where
        T: Default,
    {
        Self {
            tracker_id: UniqueId::new(),
            change_counter: 0,
            inner: T::default(),
        }
    }

    pub(crate) fn new_with_val(val: T) -> Self {
        Self {
            tracker_id: UniqueId::new(),
            change_counter: 0,
            inner: val,
        }
    }

    /// Exchange a token for the value and a new token, or `None` if nothing has changed.
    pub(crate) fn get_if_changed(
        &self,
        token: &ChangeTrackerToken<T>,
    ) -> Option<(&T, ChangeTrackerToken<T>)> {
        if let Some(tid) = token.tracker_id {
            assert_eq!(self.tracker_id, tid, "Tracker mismatch!");
        }

        let changed = match token.change_counter {
            None => true,
            Some(x) if x != self.change_counter => true,
            _ => false,
        };

        if !changed {
            None
        } else {
            Some((
                &self.inner,
                ChangeTrackerToken {
                    tracker_id: Some(self.tracker_id),
                    change_counter: Some(self.change_counter),
                    _phantom: PD,
                },
            ))
        }
    }

    /// Replace the inner value, and mark this tracker as changed.
    pub(crate) fn replace(&mut self, mut new_val: T) -> T {
        std::mem::swap(&mut new_val, &mut self.inner);
        self.change_counter += 1;
        new_val
    }

    fn get(&self) -> &T {
        &self.inner
    }

    /// Get a mutable reference to the inner value, and mark this tracker as changed.
    fn get_mut(&mut self) -> &mut T {
        self.change_counter += 1;
        &mut self.inner
    }
}

impl<T> ChangeTrackerToken<T> {
    pub fn new() -> Self {
        Self {
            tracker_id: None,
            change_counter: None,
            _phantom: PD,
        }
    }
}

impl<T> Default for ChangeTrackerToken<T> {
    fn default() -> Self {
        Self::new()
    }
}

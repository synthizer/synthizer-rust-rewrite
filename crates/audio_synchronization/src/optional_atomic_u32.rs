use crate::sync::{AtomicU32, Ordering};

/// Like `AtomicU32`, but working over `Option`.
///
/// This solves the problem that concurrent data structures sometimes need to represent None with 0, but must fit within
/// atomic types.  This cannot hold `u32::MAX`.
#[derive(Debug, Default)]
pub struct OptionalAtomicU32 {
    inner: AtomicU32,
}

fn transform_in(opt: Option<u32>) -> u32 {
    match opt {
        Some(x) => x.checked_add(1).expect("Cannot contain u32::MAX"),
        None => 0,
    }
}

fn transform_out(input: u32) -> Option<u32> {
    // checked_sub is None for 0, otherwise decreases by 1, which is what we want.
    input.checked_sub(1)
}

impl OptionalAtomicU32 {
    pub fn new(value: Option<u32>) -> Self {
        Self {
            inner: AtomicU32::new(transform_in(value)),
        }
    }

    pub fn load(&self, ordering: Ordering) -> Option<u32> {
        transform_out(self.inner.load(ordering))
    }

    pub fn store(&self, value: Option<u32>, ordering: Ordering) {
        self.inner.store(transform_in(value), ordering)
    }

    pub fn compare_exchange(
        &self,
        current: Option<u32>,
        new: Option<u32>,
        success_ordering: Ordering,
        fail_ordering: Ordering,
    ) -> Result<Option<u32>, Option<u32>> {
        let cur = transform_in(current);
        let n = transform_in(new);
        self.inner
            .compare_exchange(cur, n, success_ordering, fail_ordering)
            .map(transform_out)
            .map_err(transform_out)
    }
}

use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

/// A process-weide unique ID.
///
/// This opaque ID is unique per process per version of Synthizer, e.g. it shouldn't be exposed.  The underlying implementation is very fast, and the ID contains a niche, meaning that `Option<UniqueId>` is never bigger than the struct.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct UniqueId(NonZeroU64);

impl UniqueId {
    pub fn new() -> UniqueId {
        UniqueId(unique_u64())
    }
}

impl Default for UniqueId {
    fn default() -> Self {
        UniqueId::new()
    }
}

/// Return a process-wide unique u64 for this version of Synthizer.
///
/// This has a caveat: if two versions of Synthizer are linked in the same process, then those versions will have
/// different sets of unique integers.
fn unique_u64() -> NonZeroU64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let got = COUNTER.fetch_add(1, Ordering::Relaxed);
    NonZeroU64::new(got + 1).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note that tests are parallelized. This means that the values of unique_u64() are not stable, and so the test must
    // check them against each other.
    #[test]
    fn test_unique_u64() {
        let (id1, id2, id3) = (unique_u64(), unique_u64(), unique_u64());
        assert_ne!(id1, id2);
        assert_ne!(id1, id3);
        assert_ne!(id2, id3);
    }
}

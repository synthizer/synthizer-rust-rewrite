use std::marker::PhantomData;

use crate::sync::{AtomicU64, Ordering};

/// An atomic u32 plus a generation to prevent ABA
///
/// This atomic supports storing 32 bits of integral data, with a 32-bit generation counter.  The generation counter is
/// incremented on every operation.
///
/// As long as no two threads attempt to perform operations such as CAS such that the generation wraps around, ABA is
/// not a concern.  Practically speaking, for consumer-grade hardware, this would only be possible if threads spun for
/// unrealistically long periods of time: an X86 core circa 2023 cannot wrap the generation without performing around a
/// second of uninterrupted operations on the same atomic.
pub struct GenerationalAtomicU32 {
    storage: AtomicU64,
}

/// A value read from a [GenerationalAtomicU32], which contains the generation as well as the value.
///
/// All operations on [GenerationalAtomicU32] which return values return this type.  It is not possible to construct
/// this directly in order to ensure correct usage: it serves as a type-level proof that the contained generation
/// existed when the value was read.
///
/// The lifetime parameter exists so that it is not possible to hold a value loaded from an atomic across a
/// possibly-mutable access to that atomic.  In other words, storing these in containers is currently not supported
/// unless that container is known not to outlive the access to the atomic.  While it at first seems like this shouldn't
/// take borrows, long-lived loaded values can defeat the ABA-avoidance scheme of having a generation in the first
/// place.
#[derive(Copy, Clone, Debug)]
pub struct GenerationalAtomicU32Value<'a> {
    generation: u32,
    value: u32,
    _phantom: PhantomData<&'a u32>,
}

#[inline(always)]
fn pack_generation(generation: u32, value: u32) -> u64 {
    ((generation as u64) << 32) | (value as u64)
}

impl<'a> GenerationalAtomicU32Value<'a> {
    #[inline(always)]
    fn unpack(val: u64) -> GenerationalAtomicU32Value<'a> {
        let generation = (val >> 32).try_into().unwrap();
        let value = val as u32;
        GenerationalAtomicU32Value {
            generation,
            value,
            _phantom: PhantomData,
        }
    }

    /// Get the underlying u32 value.
    #[inline(always)]
    pub fn get(&self) -> u32 {
        self.value
    }
}

impl GenerationalAtomicU32 {
    #[cfg(not(loom))]
    pub const fn new(value: u32) -> GenerationalAtomicU32 {
        Self {
            storage: AtomicU64::new(value as u64),
        }
    }

    #[cfg(loom)]
    pub fn new(value: u32) -> GenerationalAtomicU32 {
        Self {
            storage: AtomicU64::new(value as u64),
        }
    }

    /// Set this atomic mutably.
    ///
    /// Since values are borrowed and there is a mutable reference, it is safe to call this function. This is useful
    /// when data is owned only by a single thread.
    ///
    /// If `--cfg=loom` is set, this function is not available: loom forces the atomic to be pinned in this case.
    #[cfg(not(loom))]
    pub fn get_mut(&mut self) -> &mut u32 {
        unsafe {
            let storage_mut = self.storage.get_mut() as *mut u64;

            #[cfg(target_endian = "little")]
            const OFFSET: usize = 0;
            #[cfg(target_endian = "big")]
            const OFFSET: usize = 4;

            let as_u8s = storage_mut as *mut u8;
            let at_start = as_u8s.add(OFFSET);
            &mut *(at_start as *mut u32)
        }
    }

    /// Load this atomic.  The returned value can then be used with the compare_exchange variants.
    #[inline(always)]
    pub fn load(&self, ordering: Ordering) -> GenerationalAtomicU32Value {
        let val = self.storage.load(ordering);
        GenerationalAtomicU32Value::unpack(val)
    }

    /// Like std's compare_exchange.
    #[inline(always)]
    pub fn compare_exchange(
        &self,
        current: GenerationalAtomicU32Value,
        new: u32,
        success: Ordering,
        failure: Ordering,
    ) -> Result<GenerationalAtomicU32Value, GenerationalAtomicU32Value> {
        let old_val = pack_generation(current.generation, current.value);
        let new_val = pack_generation(current.generation + 1, new);
        self.storage
            .compare_exchange(old_val, new_val, success, failure)
            .map(GenerationalAtomicU32Value::unpack)
            .map_err(GenerationalAtomicU32Value::unpack)
    }

    /// Store a u32 value.
    ///
    /// This is *not* fast and must be implemented as a CAS loop because it is necessary to get the generation before
    /// setting a new value.  Thus the name.
    pub fn store_slow(&self, value: u32, ordering: Ordering) {
        while self
            .compare_exchange(
                self.load(Ordering::Relaxed),
                value,
                ordering,
                Ordering::Relaxed,
            )
            .is_err()
        {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use proptest::{prop_assert_eq, proptest};

    proptest! {
        #[test]
        fn unpack_pack_inverses(val: u64) {
            let unpacked = GenerationalAtomicU32Value::unpack(val);
            let packed = pack_generation(unpacked.generation, unpacked.value);
            prop_assert_eq!(val, packed, "{:?}", unpacked);
        }
    }

    #[test]
    fn test_basic_cas_succeeds() {
        crate::sync::wrap_test(|| {
            let atomic = GenerationalAtomicU32::new(5);
            let loaded = atomic.load(Ordering::Relaxed);
            assert_eq!(loaded.get(), 5);
            assert!(atomic
                .compare_exchange(loaded, 6, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok());
            assert_eq!(atomic.load(Ordering::Relaxed).get(), 6);
        });
    }

    #[test]
    fn cannot_cas_backward() {
        crate::sync::wrap_test(|| {
            let atomic = GenerationalAtomicU32::new(5);
            let oldest = atomic.load(Ordering::Relaxed);
            // Invalidate oldest.
            atomic.store_slow(6, Ordering::Relaxed);
            assert!(atomic
                .compare_exchange(oldest, 7, Ordering::Relaxed, Ordering::Relaxed)
                .is_err());
            assert_eq!(atomic.load(Ordering::Relaxed).get(), 6);
        });
    }

    #[test]
    fn cannot_cas_backward_even_if_self_stored() {
        crate::sync::wrap_test(|| {
            let atomic = GenerationalAtomicU32::new(5);
            let oldest = atomic.load(Ordering::Relaxed);
            // Invalidate oldest by storing the same value, which should still move the generation.
            atomic.store_slow(5, Ordering::Relaxed);
            assert!(atomic
                .compare_exchange(oldest, 7, Ordering::Relaxed, Ordering::Relaxed)
                .is_err());
            assert_eq!(atomic.load(Ordering::Relaxed).get(), 5);
        });
    }
}

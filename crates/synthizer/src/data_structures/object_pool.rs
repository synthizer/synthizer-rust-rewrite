use std::any::{Any, TypeId};
use std::sync::Arc;
use std::sync::RwLock;

use ahash::{HashMap, HashMapExt};

use audio_synchronization::concurrent_slab::*;

/// Initial capacity for each slab in the pool.
const INITIAL_CAPACITY: u32 = 100;

/// Manages a collection of [SlabHandle]s, each for a different kind of object.
pub(crate) struct ObjectPool {
    slabs: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl ObjectPool {
    pub fn new() -> Self {
        ObjectPool {
            slabs: RwLock::new(HashMap::with_capacity(100)),
        }
    }

    fn get_or_insert_slab<T: Any + Send + Sync>(&self) -> SlabHandle<T> {
        {
            let guard = self.slabs.read().unwrap();
            if let Some(s) = guard.get(&TypeId::of::<T>()) {
                return s.downcast_ref::<SlabHandle<T>>().unwrap().clone();
            }
        }

        let mut guard = self.slabs.write().unwrap();
        if let Some(s) = guard.get(&TypeId::of::<T>()) {
            return s.downcast_ref::<SlabHandle<T>>().unwrap().clone();
        }

        let nh = SlabHandle::<T>::new(INITIAL_CAPACITY);
        guard.insert(TypeId::of::<T>(), Arc::new(nh.clone()));
        nh
    }

    pub(crate) fn allocate<T: Any + Send + Sync>(&self, new_val: T) -> ExclusiveSlabRef<T> {
        self.get_or_insert_slab::<T>().insert(new_val)
    }
}

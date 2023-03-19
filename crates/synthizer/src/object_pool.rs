use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use std::sync::Arc;

use arc_swap::ArcSwap;
use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};

type Slab<T> = sharded_slab::Slab<AtomicRefCell<T>, sharded_slab::DefaultConfig>;

/// A pool of objects where each object can be accessed either mutably by one thread, or immutably by many.
pub(crate) struct ObjectPool {
    slabs: ArcSwap<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

/// A mutable reference into an [ObjectPool].
pub(crate) struct ObjectPoolRefMut<'a, T>(AtomicRefMut<'a, T>);

/// An immutable reference into an [ObjectPool].
pub(crate) struct ObjectPoolRef<'a, T>(AtomicRef<'a, T>);

/// A token which can be presented to an [ObjectPool] to take a borrow of an object, or delete it.
pub(crate) struct ObjectPoolToken<T>(usize, PhantomData<*const T>);

/// A location in an object pool.
///
/// It is unfortunately necessary to store an Arc on the stack in order to make Rust's borrow checker happy, and it is
/// also convenient to be able to pass locations between threads.  Locations are faster than direct object pool access:
/// they have already found and dereffed the right slab.
pub(crate) struct ObjectPoolLocation<T>(sharded_slab::OwnedEntry<AtomicRefCell<T>>);

impl ObjectPool {
    pub(crate) fn new() -> Self {
        Self {
            slabs: ArcSwap::new(Arc::new(HashMap::new())),
        }
    }

    /// Get a slab for a specified type, without inserting.
    fn get_slab<T: Any + Send + Sync>(&self) -> Option<Arc<Slab<T>>> {
        let tid = TypeId::of::<T>();
        let anyarc = self.slabs.load().get(&tid)?.clone();
        Some(
            anyarc
                .downcast::<Slab<T>>()
                .expect("Internal error: entry in the slab map is not a slab"),
        )
    }

    /// Get a slab for a specified type, inserting if necessary.
    fn get_or_insert_slab<T: Any + Send + Sync>(&self) -> Arc<Slab<T>> {
        // The common case: this already contains a slab for the given type.
        if let Some(s) = self.get_slab::<T>() {
            return s;
        }

        let new_slab: Arc<dyn Any + Send + Sync> = Arc::new(Slab::<T>::new());

        self.slabs.rcu(move |m| {
            if m.contains_key(&TypeId::of::<T>()) {
                return m.clone();
            }

            let mut cloned = (**m).clone();
            cloned
                .entry(TypeId::of::<T>())
                .or_insert_with(|| new_slab.clone());
            Arc::new(cloned)
        });

        self.get_slab().unwrap()
    }

    /// Insert an object into the pool.
    ///
    /// The returned token can later be used to retrieve the object.
    ///
    /// Panics if it is not possible to insert into a sharded-slab slab due to the number of pages or max threads.  See
    /// sharded-slab docs for details on these limits; on 64-bit platforms this is definitely more than we can use
    /// (1024 active threads), and on 32-bit platforms we still get 128.
    fn insert<T: Any + Send + Sync>(&self, value: T) -> ObjectPoolToken<T> {
        let slab = self.get_or_insert_slab::<T>();
        let index = slab
            .insert(AtomicRefCell::new(value))
            .expect("Unable to allocate a new value");
        ObjectPoolToken(index, PhantomData)
    }

    /// Look up an object in the pool.
    ///
    /// The returned [ObjectPoolLocation] may then have [ObjectPoolLocation::Get] or [ObjectPoolLocation::get_mut]
    /// called on it.  Note that the underlying slab for this object type is kept alive until all locations are dropped,
    /// even if the pool is dropped first.
    ///
    /// If the token is used to delete an object from another thread, then the object will continue to exist until such
    /// time as the location is dropped.
    pub(crate) fn lookup<T: Any + Send + Sync>(
        &self,
        token: &ObjectPoolToken<T>,
    ) -> Option<ObjectPoolLocation<T>> {
        let slab = self.get_slab::<T>()?;
        let ent = slab.get_owned(token.0)?;
        Some(ObjectPoolLocation(ent))
    }

    /// Delete an entry in the pool.
    ///
    /// Panics if the entry was not first allocated in this pool.
    pub(crate) fn delete<T: Any + Send + Sync>(&self, token: ObjectPoolToken<T>) {
        self.get_slab::<T>()
            .expect("Should find the slab")
            .take(token.0)
            .expect("Should have found an entry to remove");
    }
}

impl<T: Any + Send + Sync> ObjectPoolLocation<T> {
    /// Get an immutable reference.
    ///
    /// If another thread is holding a mutable reference, this function panics.
    pub(crate) fn get(&self) -> ObjectPoolRef<'_, T> {
        ObjectPoolRef(self.0.borrow())
    }

    /// Get a mutable reference to this object.
    ///
    /// Panics if another thread holds a mutable or immutable borrow.
    pub(crate) fn get_mut(&self) -> ObjectPoolRefMut<'_, T> {
        ObjectPoolRefMut(self.0.borrow_mut())
    }

    /// turn this location back into a token.
    fn into_token(self) -> ObjectPoolToken<T> {
        let ind = self.0.key();
        ObjectPoolToken(ind, PhantomData)
    }
}

impl<'a, T> Deref for ObjectPoolRef<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a, T> Deref for ObjectPoolRefMut<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a, T> DerefMut for ObjectPoolRefMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_basic() {
        let pool = ObjectPool::new();

        let itoks = (0..3u32).map(|i| pool.insert(i)).collect::<Vec<_>>();
        let stoks = (0..3usize)
            .map(|i| pool.insert(format!("s{}", i)))
            .collect::<Vec<_>>();

        for (i, t) in itoks.iter().enumerate() {
            let loc = pool.lookup(t).expect("Should find the object");
            assert_eq!(&*loc.get(), &(i as u32));
            assert_eq!(&*loc.get_mut(), &(i as u32));
        }

        for (i, t) in stoks.iter().enumerate() {
            let expected = format!("s{}", i);
            let loc = pool.lookup(t).expect("Should find the object");
            assert_eq!(&*loc.get(), &expected);
            assert_eq!(&*loc.get_mut(), &expected);
        }

        for t in stoks {
            pool.delete(t);
        }

        for t in itoks {
            pool.delete(t);
        }
    }

    #[test]
    #[should_panic]
    fn test_no_simultaneous_borrows() {
        let pool = ObjectPool::new();

        let tok = pool.insert(1u32);
        let loc = pool.lookup(&tok).unwrap();

        let _good1 = loc.get();
        let _good2 = loc.get();
        let _panics_here = loc.get_mut();
    }
}

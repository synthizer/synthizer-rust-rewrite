//! Note: reexported from `core_traits`.
use std::marker::PhantomData as PD;

mod sealed {
    use super::*;

    /// Something that knows how to provide (possibly borrowed) values from signals to be fed to other signals.
    ///
    /// The reason for this trait is that arrays aren't really enough.  For example, slots want to borrow from the slot
    /// itself rather than having to clone out values.  This trait encapsulates the ability to do so, then adds in
    /// helper methods for getting owned values.
    ///
    /// Taking `&mut self` even when handing out shared references is not a mistake.  This lets us implement for
    /// closures, by stashing the return value of the closure and handing out a reference.
    ///
    /// # Safety
    ///
    /// Signals may:
    ///
    /// - Borrow a value more than once.
    /// - Not use all values from the provider.
    /// - Use the values in any order.
    ///
    /// Signals must not:
    ///
    /// - Go beyond the end of the number of times `tick` has been instantiated for.
    /// - Assume that `get_mut` etc. are returning mutable references to different values on every call (this exists for
    ///   e.g. slots; the idea is that it's *supposed* to be the same value a lot of the time).
    ///
    /// To implement this implement the `xxx_unsafe` methods.  The default impls for everything else should be left
    /// alone.  Note however that usage of the unsafe methods is common: the safe ones add in safety checks, but that
    /// has a performance cost.
    pub unsafe trait ValueProvider<T>: Sized {
        unsafe fn get_unchecked(&mut self, index: usize) -> &T;
        unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T;
        fn len(&mut self) -> usize;

        fn get(&mut self, index: usize) -> &T {
            if index >= self.len() {
                panic!("Index out of bounds");
            }

            unsafe { self.get_unchecked(index) }
        }

        fn get_mut(&mut self, index: usize) -> &mut T {
            if index >= self.len() {
                panic!("Index out of bounds");
            }

            unsafe { self.get_unchecked_mut(index) }
        }

        fn get_cloned(&mut self, index: usize) -> T
        where
            T: Clone,
        {
            if index >= self.len() {
                panic!("Index out of bounds");
            }

            unsafe { self.get_unchecked(index) }.clone()
        }

        fn iter_cloned(mut self) -> ValueProviderIterator<T, Self>
        where
            Self: Sized,
        {
            ValueProviderIterator {
                index: 0,
                max_index: self.len(),
                provider: self,
                _phantom: PD,
            }
        }
    }

    pub struct ValueProviderIterator<T, P> {
        provider: P,
        index: usize,
        max_index: usize,
        _phantom: PD<T>,
    }

    impl<T, P> std::iter::Iterator for ValueProviderIterator<T, P>
    where
        P: ValueProvider<T>,
        T: Clone,
    {
        type Item = T;

        fn next(&mut self) -> Option<Self::Item> {
            if self.index == self.max_index {
                None
            } else {
                self.index += 1;
                Some(self.provider.get_cloned(self.index - 1))
            }
        }
    }
}

pub(crate) use sealed::*;

pub(crate) struct ClosureProvider<T, F, const LEN: usize> {
    closure: F,
    temp_storage: Option<T>,
}

unsafe impl<T, F, const LEN: usize> ValueProvider<T> for ClosureProvider<T, F, LEN>
where
    F: FnMut(usize) -> T,
{
    unsafe fn get_unchecked(&mut self, index: usize) -> &T {
        self.temp_storage = Some((self.closure)(index));
        self.temp_storage.as_ref().unwrap()
    }

    unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T {
        self.temp_storage = Some((self.closure)(index));
        self.temp_storage.as_mut().unwrap()
    }

    fn len(&mut self) -> usize {
        LEN
    }
}

impl<T, F, const LEN: usize> ClosureProvider<T, F, LEN> {
    pub(crate) fn new(closure: F) -> Self {
        Self {
            closure,
            temp_storage: None,
        }
    }
}

pub(crate) struct ArrayProvider<T, const LEN: usize> {
    array: [Option<T>; LEN],
}

impl<T, const LEN: usize> ArrayProvider<T, LEN> {
    pub(crate) fn new(array: [T; LEN]) -> Self {
        Self {
            array: array.map(Some),
        }
    }
}

unsafe impl<T, const LEN: usize> ValueProvider<T> for ArrayProvider<T, LEN> {
    unsafe fn get_unchecked(&mut self, index: usize) -> &T {
        self.array[index].as_ref().unwrap()
    }

    unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T {
        self.array[index].as_mut().unwrap()
    }

    fn len(&mut self) -> usize {
        LEN
    }
}

pub(crate) struct FixedValueProvider<T, const LEN: usize> {
    value: T,
}

impl<T, const LEN: usize> FixedValueProvider<T, LEN> {
    pub(crate) fn new(value: T) -> Self {
        Self { value }
    }
}

unsafe impl<T, const LEN: usize> ValueProvider<T> for FixedValueProvider<T, LEN>
where
    T: Clone,
{
    fn len(&mut self) -> usize {
        LEN
    }

    unsafe fn get_unchecked(&mut self, _index: usize) -> &T {
        &self.value
    }

    unsafe fn get_unchecked_mut(&mut self, _index: usize) -> &mut T {
        &mut self.value
    }
}

use std::sync::Arc;

use arc_swap::ArcSwapOption;

// This is a singley-linked list which may be prepended to, backed by arc_swap.  It does not use loom for testing, since
// we either trust arc_swap or we don't and in addition loom can't see through arc_swap anyway.

/// A singley-linked list which may only be prepended to.
///
/// Iteration over this list returns references to [Arc] which point at the interior items of the list.  This does
/// unfortunately involve double pointer chasing.  This list is useful when the items are large and frequently used in
/// small number, as happens in e.g. [crate::concurrent_slab::ConcurrentSlab].
pub struct PrependOnlyList<T: Send + Sync> {
    head: ArcSwapOption<Node<T>>,
}

struct Node<T: Send + Sync> {
    next: ArcSwapOption<Node<T>>,
    value: Arc<T>,
}

impl<T: Send + Sync> PrependOnlyList<T> {
    pub fn new() -> Self {
        Default::default()
    }

    /// Prepend the given Arc to this list.
    ///
    /// This allocates.
    pub fn prepend(&self, value: Arc<T>) {
        let node = Arc::new(Node {
            next: ArcSwapOption::new(None),
            value,
        });

        self.head.rcu(move |x| {
            node.next.store(x.clone());
            Some(node.clone())
        });
    }

    /// Iterate over the entries of this list.
    pub fn iter<'a>(&'a self) -> impl Iterator<Item = &'a Arc<T>> + 'a {
        use std::marker::PhantomData;

        // We can most easily implement this ourselves with a hidden struct.
        struct HiddenIter<'i, T: Send + Sync> {
            maybe_node: Option<Arc<Node<T>>>,
            _phantom: PhantomData<&'i T>,
        }

        impl<'a, T: Send + Sync> Iterator for HiddenIter<'a, T> {
            type Item = &'a Arc<T>;

            fn next(&mut self) -> Option<Self::Item> {
                let node = self.maybe_node.as_ref()?;

                // First, if we have a node, let's figure out our return value.  The actual thing responsible for
                // keeping references around alive is the list, and this struct's lifetime is tied to the list even
                // though it does not hold a reference to the list.  That means that it is safe to extend the reference
                // since the list is prepend-only.  Unfortunately, this means that we must temporarily round-trip
                // through a pointer; this borrow is a field inside the Arc that is node, which is kept alive by the
                // list and not by us, and so the referent is not invalidated when it's uipdated.
                let ret = &node.value as *const Arc<T>;

                self.maybe_node = node.next.load_full();

                Some(unsafe { ret.as_ref().unwrap_unchecked() })
            }
        }

        HiddenIter::<'a, T> {
            maybe_node: self.head.load_full(),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T: Send + Sync> Default for PrependOnlyList<T> {
    fn default() -> Self {
        PrependOnlyList {
            head: ArcSwapOption::new(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iteration() {
        let list = PrependOnlyList::<u32>::new();

        for i in (0..10).rev() {
            list.prepend(Arc::new(i));
        }

        let entries = list.iter().map(|x| **x).collect::<Vec<u32>>();
        assert_eq!(entries, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}

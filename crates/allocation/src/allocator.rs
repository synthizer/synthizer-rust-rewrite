use crate::allocation_strategies::*;
use crate::shared_ptr::*;

/// An allocator for [SharedPtr]s.
///
/// This allocator can either grab [SharedPtr]s backed by raw boxes, or alternatively paged allocations for smaller objects.  See [AllocatorConfig] for the knobs.
pub struct Allocator {
    boxed_strategy: SimpleBoxStrategy,
    paged_strategy: PagedStrategy,
    config: AllocatorConfig,
}

/// Configuration for an [Allocator].
///
/// the default values are tuned to page objects of 1KiB size on 1 MiB pages.
#[derive(Debug, Clone)]
pub struct AllocatorConfig {
    /// What is the number of elements to put on a page?
    ///
    /// Default is 1024.
    pub page_elements: usize,

    /// What is the maximum size of a page in bytes?
    ///
    /// If it is not possible to get enough elements on the page, the allocator switches to using boxes.
    ///
    /// Default is 1MiB.
    pub page_size: usize,
}

impl Default for AllocatorConfig {
    fn default() -> Self {
        AllocatorConfig {
            page_size: 1 << 20,
            page_elements: 1024,
        }
    }
}

const CONTROL_BLOCK_PAGE_ELEMENTS: usize = 5 << 10;

impl Allocator {
    pub fn new(config: AllocatorConfig) -> Allocator {
        Allocator {
            boxed_strategy: SimpleBoxStrategy,
            paged_strategy: PagedStrategy::new(CONTROL_BLOCK_PAGE_ELEMENTS, config.page_elements),
            config,
        }
    }

    /// Allocate a shared pointer for a `T` according to the config for this allocator.
    pub fn allocate<T: Send + Sync + 'static>(&self, val: T) -> SharedPtr<T> {
        let type_size = std::mem::size_of::<T>();

        if type_size == 0 {
            return SharedPtr::new_zst();
        }

        let possible_elements = self.config.page_size / type_size;
        if possible_elements <= self.config.page_elements {
            SharedPtr::new(&self.paged_strategy, val)
        } else {
            SharedPtr::new(&self.boxed_strategy, val)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ZeroSized;

    impl ZeroSized {
        fn hello(&self) -> &str {
            "hello"
        }
    }

    #[test]
    fn test_alloc_zst() {
        let alloc = Allocator::new(Default::default());
        let got = alloc.allocate(ZeroSized);
        assert_eq!(got.hello(), "hello");
    }
}

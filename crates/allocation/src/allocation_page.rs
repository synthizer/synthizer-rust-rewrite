use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

/// A page of allocated data.
pub(crate) struct AllocationPage<T: 'static> {
    items: Vec<UnsafeCell<MaybeUninit<T>>>,

    /// bits are set in this bitset as the page fills, and cleared as the page is emptied.
    ///
    /// The index of a given bit is `BIT % 64`, then numbered starting at the LSB.
    bitset: Vec<AtomicU64>,
}

impl<T: 'static> AllocationPage<T> {
    pub(crate) fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        let bitset_size = capacity / 64 + (capacity % 64 != 0) as usize;
        let mut ap = AllocationPage {
            items: Vec::new(),
            bitset: Vec::new(),
        };

        ap.items
            .resize_with(capacity, || UnsafeCell::new(MaybeUninit::uninit()));
        ap.bitset.resize_with(bitset_size, || AtomicU64::new(0));

        ap
    }

    /// Find a possibly free index in the page to allocate.
    fn find_index(&self) -> Option<usize> {
        for i in 0..self.bitset.len() {
            let base = i * 64;
            let val = self.bitset[i].load(Ordering::Relaxed);
            let trailing = val.trailing_ones();

            if trailing == 64 {
                continue;
            }

            if base + trailing as usize >= self.items.len() {
                return None;
            } else {
                return Some(base + trailing as usize);
            }
        }

        None
    }

    /// try to allocate the given index, returning false if this is not possible.
    fn alloc_index(&self, index: usize) -> bool {
        let bitset_index = index / 64;
        let bitset_offset = index % 64;
        let mask: u64 = 1 << bitset_offset as usize;

        let mut cur_val = self.bitset[bitset_index].load(Ordering::Relaxed);

        // the bit gets set (and the mask will become nonzero) when allocated.
        while cur_val & mask == 0 {
            let new_val = cur_val | mask;
            match self.bitset[bitset_index].compare_exchange(
                cur_val,
                new_val,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    // We got it.
                    return true;
                }
                Err(x) => {
                    cur_val = x;
                }
            }
        }

        false
    }

    /// Attempt to allocate a pointer from this page for the given `T`, returning a raw pointer to it.
    ///
    /// Returns `None` if there was no more capacity.
    pub(crate) fn allocate(&self, val: T) -> Result<NonNull<T>, T> {
        while let Some(ind) = self.find_index() {
            if !self.alloc_index(ind) {
                continue;
            }
            unsafe {
                let ptr = self.items[ind].get();
                ptr.as_mut().unwrap().write(val);
                return Ok(NonNull::new(ptr as *mut T).unwrap());
            }
        }

        Err(val)
    }

    /// Given a pointer whichb was allocated in this page, deallocate the pointer and drop the data.
    pub(crate) fn deallocate(&self, ptr: NonNull<T>) {
        let ptr_usize = ptr.as_ptr() as usize;
        let start_usize = self.items[0].get() as usize;
        let diff_usize = ptr_usize
            .checked_sub(start_usize)
            .expect("Should be in the page");
        let ind = diff_usize / std::mem::size_of::<T>();
        assert!(
            ind < self.items.len(),
            "The pointer passed in should be inside the page"
        );

        unsafe {
            self.items[ind].get().as_mut().unwrap().assume_init_drop();
        }

        self.free_index(ind);
    }

    fn free_index(&self, index: usize) {
        let bitset_ind = index / 64;
        let bitset_offset = index % 64;
        let mask = 1 << bitset_offset;

        loop {
            let cur_val = self.bitset[bitset_ind].load(Ordering::Relaxed);
            assert!(cur_val & mask != 0);
            let new_val = cur_val & !mask;
            if self.bitset[bitset_ind]
                .compare_exchange(cur_val, new_val, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }

            #[cfg(loom)]
            loom::thread::yield_now();
        }
    }
}

unsafe impl<T> Sync for AllocationPage<T> {}
unsafe impl<T> Send for AllocationPage<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    use crossbeam::channel as chan;

    #[test]
    fn test_basic_single_threaded() {
        let page: AllocationPage<u32> = AllocationPage::new(63);
        let mut ptrs = vec![];

        for i in 0..63u32 {
            ptrs.push(
                page.allocate(i)
                    .expect("Should always allocate at this point"),
            );
        }

        for (i, p) in ptrs.iter().enumerate().map(|(a, b)| (a, *b)) {
            let got = unsafe { *p.as_ref() };
            assert_eq!(got, i as u32);
        }

        // Deallocating a pointer and reallocating it should put it back.
        page.deallocate(ptrs[3]);
        assert_eq!(page.allocate(2).unwrap(), ptrs[3]);

        // And we can't go beyond the end.
        assert!(page.allocate(500).is_err());

        assert_eq!(page.bitset[0].load(Ordering::Relaxed), u64::MAX / 2);
    }

    #[test]
    fn test_basic_multithreaded() {
        use std::sync::Arc;

        let page = Arc::new(AllocationPage::<u32>::new(256));

        let mut handles = vec![];
        for i in 0..4 {
            let page = page.clone();
            let jh = std::thread::spawn(move || {
                let offset = i * 64;

                let mut ptrs = vec![];

                for i in offset..(offset + 64) {
                    ptrs.push(
                        page.allocate(i as u32)
                            .expect("Should always allocate at this point"),
                    );
                }

                for (i, p) in ptrs.iter().enumerate().map(|(a, b)| (a, *b)) {
                    let got = unsafe { *p.as_ref() };
                    assert_eq!(got, (i + offset) as u32);
                }

                // Now lets bash on the allocation and deallocation by reallocating them all pairwise.
                for (i, dest) in ptrs.iter_mut().enumerate() {
                    page.deallocate(*dest);
                    *dest = page
                        .allocate((i + offset) as u32)
                        .expect("Should be able to allocate");
                    let got = unsafe { *dest.as_ref() };
                    assert_eq!(got, (i + offset) as u32);
                }

                for p in ptrs.iter() {
                    page.deallocate(*p);
                }
            });
            handles.push(jh);
        }

        for h in handles {
            h.join().unwrap();
        }
    }

    #[derive(Debug)]
    struct DropRecorder(chan::Sender<usize>, usize);

    impl Drop for DropRecorder {
        fn drop(&mut self) {
            self.0.send(self.1).unwrap();
        }
    }

    #[test]
    fn test_multithreaded_drop() {
        use std::sync::Arc;

        let (sender, receiver) = chan::unbounded::<usize>();
        let (expected_sender, expected_receiver) = chan::unbounded::<usize>();

        let page = Arc::new(AllocationPage::<DropRecorder>::new(256));

        let mut handles = vec![];
        for i in 0..4 {
            let sender = sender.clone();
            let expected_sender = expected_sender.clone();
            let page = page.clone();

            let jh = std::thread::spawn(move || {
                let offset = i * 64;

                let mut ptrs = vec![];

                for i in 0..64 {
                    ptrs.push(
                        page.allocate(DropRecorder(sender.clone(), offset + i))
                            .expect("Should always allocate at this point"),
                    );
                }

                for (i, p) in ptrs.iter().enumerate().map(|(a, b)| (a, *b)) {
                    let got = unsafe { p.as_ref().1 };
                    assert_eq!(got, i + offset);
                }

                // Now lets bash on the allocation and deallocation by reallocating them all pairwise.
                for (i, dest) in ptrs.iter_mut().enumerate() {
                    page.deallocate(*dest);
                    expected_sender.send(offset + i).unwrap();
                    *dest = page
                        .allocate(DropRecorder(sender.clone(), i + offset))
                        .expect("Should be able to allocate");
                    let got = unsafe { dest.as_ref().1 };
                    assert_eq!(got, i + offset);
                }

                for (i, p) in ptrs.iter().enumerate() {
                    page.deallocate(*p);
                    expected_sender.send(i + offset).unwrap();
                }
            });
            handles.push(jh);
        }

        for h in handles {
            h.join().unwrap();
        }

        // Close the channel so that we won't block forever when collecting to vecs.
        std::mem::drop(sender);
        std::mem::drop(expected_sender);

        let mut expected = expected_receiver.into_iter().collect::<Vec<_>>();
        let mut got = receiver.into_iter().collect::<Vec<_>>();
        expected.sort_unstable();
        got.sort_unstable();
        assert_eq!(got, expected);
    }
}

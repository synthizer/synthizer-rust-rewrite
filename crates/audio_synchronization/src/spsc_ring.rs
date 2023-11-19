//! An SPSC ringbuffer.
//!
//! The advantage of this over e.g. thingbuf, which is equally as fast, is that it is capable of handing out contiguous
//! slices.  This makes it particularly useful for sending things like f32 and f64 between threads.  DSP algorithms may
//! simply fil out the available space, and only take the penalty of some atomic reads and writes at the beginning/end
//! of processing.
//!
//! After u64::MAX items are passed through this ring, it will panic.
//!
//! This ring intentionally allocates uninitialized memory, and will hand out slices to such.  To deal with this, it is
//! only possible to get a slice if the element implements [bytemuck::AnyBitPattern].
//!
//! Additionally, this currently requires [Copy] as well.  We might lift that restriction in the future, but given the
//! intended use, it's simpler for now to just disallow Drop.
//!
//! No special optimization applies to powers of 2. We use the [reciprocal] crate for optimized division instead which
//! is more than fast enough provided that users use the slice-based methods.
use std::alloc::{alloc, Layout};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

use bytemuck::AnyBitPattern;
use crossbeam::utils::CachePadded;

// Implementation:
//
// This is like crossbeam channels: there is a reader half and a writer half.  Both halves may return slices and both
// halves may be told to consume some number of elements (e.g. partial writes to a slice are possible).
//
// It is implemented with the standard read and write pointers, `read <= write`, but instead of wrapping around we use
// u64.  This means that `write - read` is equal to the written elements, and `capacity - (write - read)` is equal to
// what's available for writing.

/// The ring implementation. The data is allocated immediately after this struct.
#[repr(C)]
struct Ring<T> {
    immutable: CachePadded<RingImmutableFields>,
    read_pointer: CachePadded<AtomicU64>,
    write_pointer: CachePadded<AtomicU64>,
    _phantom: std::marker::PhantomData<T>,
}

#[repr(C)]
struct RingImmutableFields {
    refcount: AtomicU64,
    divider: reciprocal::Reciprocal,
    capacity: usize,
}

/// Returns (ring_layout, offset_to_data).
#[inline(always)]
fn layout_for_ring<T>(capacity: usize) -> (Layout, usize) {
    let ring_layout = Layout::new::<Ring<T>>();
    let array_layout = Layout::array::<T>(capacity).unwrap();
    ring_layout.extend(array_layout).unwrap()
}

impl<T: AnyBitPattern> Ring<T> {
    fn new(capacity: usize) -> *mut Self {
        assert!(capacity != 0);
        let (layout, _) = layout_for_ring::<T>(capacity);
        let full_ptr = unsafe { alloc(layout) };
        let ret = full_ptr as *mut Self;
        unsafe {
            ret.write(Ring {
                immutable: CachePadded::new(RingImmutableFields {
                    refcount: AtomicU64::new(2),
                    divider: reciprocal::Reciprocal::new(capacity as u64).unwrap(),
                    capacity,
                }),
                read_pointer: CachePadded::new(AtomicU64::new(0)),
                write_pointer: CachePadded::new(AtomicU64::new(0)),
                _phantom: std::marker::PhantomData,
            });
        }
        ret
    }

    unsafe fn dec_refcount(ring: NonNull<Self>) {
        unsafe {
            let old = ring
                .as_ref()
                .immutable
                .refcount
                .fetch_sub(1, Ordering::Relaxed);
            if old == 1 {
                let (layout, _) = layout_for_ring::<T>(ring.as_ref().capacity());
                std::alloc::dealloc(ring.as_ptr() as *mut u8, layout);
            }
        }
    }

    fn both_sides_alive(&self) -> bool {
        self.immutable.refcount.load(Ordering::Relaxed) == 2
    }

    fn capacity(&self) -> usize {
        self.immutable.capacity
    }

    /// Perform `index % capacity`, but efficiently.
    fn wrap_index(&self, index: u64) -> usize {
        let whole_count = self.immutable.divider.apply(index);
        let whole_part = whole_count * (self.capacity() as u64);
        let remainder = index - whole_part;
        debug_assert!(remainder <= usize::MAX as u64);
        remainder as usize
    }

    fn data_ptr(&self) -> *mut T {
        // safety: &self is the ring, we are returning a mutable pointer to *after* the reference.
        let (_, off) = layout_for_ring::<T>(self.capacity());
        let ptr = self as *const Self as *mut Self as *mut u8;
        unsafe { ptr.add(off) as *mut T }
    }

    fn available_for_read(&self) -> usize {
        let write_ptr = self.write_pointer.load(Ordering::Relaxed);
        let read_ptr = self.read_pointer.load(Ordering::Relaxed);
        write_ptr.checked_sub(read_ptr).unwrap().try_into().unwrap()
    }

    fn available_for_write(&self) -> usize {
        let read_avail = self.available_for_read();
        let cap = self.capacity();
        cap - read_avail
    }

    unsafe fn read_one(&self) -> Option<T> {
        // We must synchronize with the write pointer, maintained by the writer.
        //
        // Also, we cannot go through available_for_read; we must not fetch-add the read pointer until the read is
        // completed.  Consequently, we must prepare both here and will simply do the computations ourself.
        let write_ptr = self.write_pointer.load(Ordering::Acquire);
        let read_ptr = self.read_pointer.load(Ordering::Relaxed);

        // write_ptr is where the writer is about to write, not the last thing the writer wrote; if read_ptr ==
        // write_ptr the ring is empty.
        if read_ptr >= write_ptr {
            return None;
        }

        // Do the read.
        let ret = Some(unsafe { self.data_ptr().add(self.wrap_index(read_ptr)).read() });
        self.read_pointer.fetch_add(1, Ordering::Relaxed);
        ret
    }

    unsafe fn write_one(&self, item: T) -> bool {
        let data_ptr = self.data_ptr();

        // Careful: we must perform a release operation after the write. We cannot fetch_add until the data is written,
        // but we can at least do a relaxed load since we do not read the memory.
        let write_ptr = self.write_pointer.load(Ordering::Relaxed);
        let read_ptr = self.read_pointer.load(Ordering::Relaxed);

        // if write_ptr >= read_ptr + capacity, then moving one more into the future would wrap the write pointer over the read pointer.
        if (write_ptr - read_ptr) >= self.capacity() as u64 {
            return false;
        }

        let cur_ind = self.wrap_index(write_ptr);
        unsafe { data_ptr.add(cur_ind).write(item) };
        let new_ind = self.write_pointer.fetch_add(1, Ordering::Release);
        assert!(new_ind >= write_ptr, "new_ind={new_ind}, cur_ind={cur_ind}");
        true
    }

    unsafe fn read_slices(&self) -> (Option<&[T]>, Option<&[T]>) {
        // We get everything which is available for reading, and return that.

        let read_ptr = self.read_pointer.load(Ordering::Relaxed);
        let write_ptr = self.write_pointer.load(Ordering::Acquire);
        let avail_read = write_ptr - read_ptr;

        if avail_read == 0 {
            return (None, None);
        }

        // Care must be taken to use avail_read to figure out our slice lengths.  If the read pointer is "left" of the
        // write pointer, then we must not start a slice at the write pointer.  If our read pointer is to the right of
        // the write pointer and the write pointer is at index 0, then there is no leftmost slice. In either case, using
        // read_avail makes this correct.

        let data_ptr = self.data_ptr();

        // First, we get the read pointer wrapped...
        let read_ptr_wrapped = self.wrap_index(read_ptr);

        // Our first slice happens if the read pointer isn't at the end of the ring. Since read pointers always point
        // into the ring and additionally we know there is stuff to read, we know that the first slice will be at least
        // one element.
        let first = {
            let slice_len = (self.capacity() - read_ptr_wrapped).min(avail_read as usize);
            let slice =
                unsafe { std::slice::from_raw_parts(data_ptr.add(read_ptr_wrapped), slice_len) };
            slice
        };
        debug_assert!(!first.is_empty());

        let second = if first.len() < avail_read as usize {
            // If there's a second slice it always starts at the beginning of the ring.
            Some(unsafe { std::slice::from_raw_parts(data_ptr, avail_read as usize - first.len()) })
        } else {
            None
        };

        (Some(first), second)
    }

    unsafe fn read_commit(&self, count: usize) {
        let avail_read = self.available_for_read();
        assert!(avail_read >= count);

        let old_ptr = self.read_pointer.fetch_add(count as u64, Ordering::Relaxed);
        old_ptr.checked_add(count as u64).expect("The ring wrapped");
    }

    unsafe fn write_slices(&self) -> (Option<&mut [T]>, Option<&mut [T]>) {
        // This is almost the same as read_slices, but with two differences. First, we do not need to synchronize with
        // the reader, which does no reads. Second, the returned slices are mutable.
        let write_ptr = self.write_pointer.load(Ordering::Relaxed);
        let read_ptr = self.read_pointer.load(Ordering::Relaxed);
        let avail_for_read = write_ptr - read_ptr;
        // Ok, but we can write the leftover bit.
        let avail_for_write = self.capacity() - avail_for_read as usize;

        if avail_for_write == 0 {
            return (None, None);
        }

        let data_ptr = self.data_ptr();

        let write_ptr_wrapped = self.wrap_index(write_ptr);

        // Like with reading, we go "to the right" of the write pointer and back around.
        let first = {
            let slice_len = (self.capacity() - write_ptr_wrapped).min(avail_for_write);
            unsafe { std::slice::from_raw_parts_mut(data_ptr.add(write_ptr_wrapped), slice_len) }
        };

        let second = if first.len() < avail_for_write {
            let remaining = avail_for_write - first.len();
            Some(unsafe { std::slice::from_raw_parts_mut(data_ptr, remaining) })
        } else {
            None
        };

        (Some(first), second)
    }

    unsafe fn write_commit(&self, count: usize) {
        debug_assert!(count <= self.available_for_write());

        let old_ptr = self
            .write_pointer
            .fetch_add(count as u64, Ordering::Release);
        old_ptr.checked_add(count as u64).expect("The ring wrapped");
    }
}

/// A reader for an SPSC ring.
pub struct RingReader<T: AnyBitPattern + Copy + Send + 'static> {
    ring: NonNull<Ring<T>>,
}

/// A writer for an SPSC ring.
pub struct RingWriter<T: AnyBitPattern + Copy + Send + 'static> {
    ring: NonNull<Ring<T>>,
}

/// Allocate a ring with a given capacity.
///
/// # Panics
///
/// Panics if `capacity == 0`.
pub fn create_ring<T: AnyBitPattern + Copy + Send + 'static>(
    capacity: usize,
) -> (RingReader<T>, RingWriter<T>) {
    let ring_ptr = Ring::new(capacity);
    unsafe {
        (
            RingReader {
                ring: NonNull::new_unchecked(ring_ptr),
            },
            RingWriter {
                ring: NonNull::new_unchecked(ring_ptr),
            },
        )
    }
}

impl<T: AnyBitPattern + Copy + Send + 'static> RingReader<T> {
    /// Read a single item from the ring.
    ///
    /// Returns either `Some(item)` or `None`. Does not block.
    ///
    /// This method is considerably slow.  Consider [RingReader::read_slices] instead.
    ///
    /// # Panics
    ///
    /// Panics if more than `u64::MAX` items have gone through this ring.
    pub fn read_one(&mut self) -> Option<T> {
        unsafe { self.ring.as_ref().read_one() }
    }

    /// Read this ring as slices, processing some number of items.
    ///
    /// This function calls the provided closure with two arguments, which form a contiguous slice.  If it is possible
    /// to get all data in the ring at once, then this closure will be called as `Some((Slice, None))`.  Otherwise, it
    /// will be called as either `None` (no data), or `Some((slice, Some(slice)))` (two slices were needed).
    ///
    /// The closure then returns the number of items it processed.  For example, if it returns 0 then it will see all of
    /// these items again along with whatever was written since.
    ///
    /// Returns the number of items the closure claimed to process.
    ///
    /// As a special case, if both the reader and the writer always work in some divisor of the ring's capacity such
    /// that both size always process that number of elements, then the first slice will always contain at least a block
    /// of data, and the sum of the lengths of both slices will be a multiple of that divisor.
    ///
    /// # Panics
    ///
    /// Panics if:
    ///
    /// - The ring has had more than `u64::MAX` items.
    /// - The closure returns that it processed more items than were available to it.
    pub fn read_slices<F: FnOnce(Option<(&[T], Option<&[T]>)>) -> usize>(
        &mut self,
        closure: F,
    ) -> usize {
        let (first, second) = unsafe { self.ring.as_ref().read_slices() };
        let avail = first.map(|x| x.len()).unwrap_or(0) + second.map(|x| x.len()).unwrap_or(0);
        let processed = match first {
            None => closure(None),
            Some(f) => closure(Some((f, second))),
        };

        assert!(processed <= avail);
        unsafe {
            self.ring.as_ref().read_commit(processed);
        }

        processed
    }

    /// Return whether or not this ring still has a writing half.
    ///
    /// Proper use of this method is tricky.  In particular, it returning false does not mean there is no more data.
    /// Proper use will call it, have it return false, and then drain the ring before dropping.
    ///
    /// When both the reader and writer halves of the ring go away, the ring itself is deallocated. For this reason, it
    /// is useful to drop this half of the ring when no other half still exists.
    pub fn has_writer(&mut self) -> bool {
        unsafe { self.ring.as_ref().both_sides_alive() }
    }

    /// Return a *hint* saying how much data is available for reading.
    ///
    /// The actual amount which is readable is `>=` this value.
    pub fn available(&mut self) -> usize {
        unsafe { self.ring.as_ref().available_for_read() }
    }

    /// Convenience method which will read as much as possible into the given slice.  Returns how much was read.
    pub fn read_to_slice(&mut self, slice: &mut [T]) -> usize {
        // For this trivial case, let's not even bother the ring.
        if slice.is_empty() {
            return 0;
        };

        self.read_slices(|slices| {
            let Some((first, second)) = slices else {
                return 0;
            };

            let mut remaining = slice.len();
            let in_first = first.len().min(remaining);
            slice[..in_first].copy_from_slice(&first[..in_first]);
            remaining -= in_first;

            let Some(second) = second else {
                return in_first;
            };

            let in_second = remaining.min(second.len());
            slice[in_first..in_first + in_second].copy_from_slice(&second[..in_second]);
            in_first + in_second
        })
    }
}

impl<T: AnyBitPattern + Copy + Send + 'static> RingWriter<T> {
    /// Write a single item into the ring, if possible.
    ///
    /// Returns true if the item was written.
    ///
    /// This function is considerably slow.  Consider [RingWriter::write_slices] instead.
    ///
    /// # Panics
    ///
    /// Panics if more than `u64::MAX` items have gone through this ring.
    pub fn write_one(&mut self, item: T) -> bool {
        unsafe { self.ring.as_ref().write_one(item) }
    }

    /// Write to this ring by writing to slices
    ///
    /// This function calls the provided closure with two arguments, which form a contiguous slice.  If it is possible
    /// to write all data in the ring at once, then this closure will be called as `Some((Slice, None))`.  Otherwise, it
    /// will be called as either `None` (no space), or `Some((slice, Some(slice)))` (two slices were needed).
    ///
    /// The closure then returns the number of items it processed.  For example, if it returns 0 then the writer will
    /// not advance, and nothing will be made available to the reader.
    ///
    /// Returns the number of items the closure said it processed.
    ///
    /// As a special case, if both the reader and the writer always work in some divisor of the ring's capacity such
    /// that both sides only ever process that many elements at a time, then the first slice will always have at least
    /// that many elements to be written, and the total sum of available space will be a multiple of that divisor.
    ///
    /// # Panics
    ///
    /// Panics if:
    ///
    /// - The ring has had more than `u64::MAX` items.
    /// - The closure returns that it processed more items than were available to it.
    pub fn write_slices<F: FnOnce(Option<(&mut [T], Option<&mut [T]>)>) -> usize>(
        &mut self,
        closure: F,
    ) -> usize {
        let (first, second) = unsafe { self.ring.as_ref().write_slices() };
        let avail = first.as_ref().map(|x| x.len()).unwrap_or(0)
            + second.as_ref().map(|x| x.len()).unwrap_or(0);

        let processed = match first {
            None => closure(None),
            Some(f) => closure(Some((f, second))),
        };

        assert!(processed <= avail);

        unsafe {
            self.ring.as_ref().write_commit(processed);
        }

        processed
    }

    /// Return whether a reader exists which will consume written data.
    ///
    /// Proper use of this method is tricky, but less tricky than [RingReader::has_writer].  When this method returns false, no data will be read so it is okay to drop this half of the buffer.  Writing further is possible, but any such data will not be consumed, and the buffer stops allowing writes once full.
    pub fn has_reader(&mut self) -> bool {
        unsafe { self.ring.as_ref().both_sides_alive() }
    }

    /// Convenience method which will return a *hint* as to how much space is available for writing.
    ///
    /// An attempt to write will always see at least this amount of space, but may see more.
    pub fn available(&mut self) -> usize {
        unsafe { self.ring.as_ref().available_for_write() }
    }

    pub fn write_from_slice(&mut self, slice: &[T]) -> usize {
        self.write_slices(|slices| {
            let Some((first, second)) = slices else {
                return 0;
            };

            let mut remaining = slice.len();
            let in_first = first.len().min(remaining);
            first[..in_first].copy_from_slice(&slice[..in_first]);

            remaining -= in_first;

            let Some(second) = second else {
                return in_first;
            };

            let in_second = remaining.min(second.len());
            second[..in_second].copy_from_slice(&slice[in_first..(in_first + in_second)]);
            in_first + in_second
        })
    }
}

impl<T: AnyBitPattern + Copy + Send + 'static> Drop for RingReader<T> {
    fn drop(&mut self) {
        unsafe { Ring::dec_refcount(self.ring) }
    }
}

impl<T: AnyBitPattern + Copy + Send + 'static> Drop for RingWriter<T> {
    fn drop(&mut self) {
        unsafe { Ring::dec_refcount(self.ring) }
    }
}

unsafe impl<T: AnyBitPattern + Send + 'static> Send for RingReader<T> {}
unsafe impl<T: AnyBitPattern + Send + 'static> Send for RingWriter<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    use std::thread::spawn;

    // Note on our testing strategy:
    //
    // Loom really hates what is in effect spinlocks, and this ring is entirely that. We thus cannot easily use Loom.
    // Instead, we have to hammer on it via traditional threads, and careful asserting that things will work out.

    #[test]
    fn test_write_read_simple() {
        let (mut reader, mut writer) = create_ring(5);

        for i in 0..10u64 {
            writer.write_one(i);
            assert_eq!(reader.read_one(), Some(i));
        }
    }

    #[test]
    fn test_write_read_simple_multithreaded() {
        let (mut reader, mut writer) = create_ring(5);

        let bg_thread = spawn(move || {
            for i in 0..100000u64 {
                while !writer.write_one(i) {
                    std::thread::yield_now();
                }
            }
        });

        for i in 0..100000u64 {
            loop {
                if let Some(x) = reader.read_one() {
                    assert_eq!(x, i);
                    break;
                }
            }
        }

        bg_thread.join().unwrap();
    }

    /// Test that if one ever only reads and writes slices which are a divisor of the capacity, one only gets blocks.
    #[test]
    fn test_slices_are_always_blocks_simple() {
        let ints = (0..10).collect::<Vec<u64>>();
        let (mut reader, mut writer) = create_ring::<u64>(20);

        for _ in 0..5 {
            writer.write_slices(|slices| {
                let (first, second) = slices.unwrap();
                assert_eq!(first.len() % ints.len(), 0);
                assert_eq!(
                    (first.len() + second.map(|x| x.len()).unwrap_or(0)) % ints.len(),
                    0
                );
                first[..ints.len()].copy_from_slice(&ints[..]);
                ints.len()
            });
            reader.read_slices(|slices| {
                let (first, second) = slices.unwrap();
                assert_eq!(first.len() % ints.len(), 0);
                assert_eq!(
                    (first.len() + second.map(|x| x.len()).unwrap_or(0)) % ints.len(),
                    0
                );
                assert_eq!(first, &ints[..]);
                ints.len()
            });
        }
    }

    #[test]
    fn test_slice_processing_multithreaded() {
        struct Opts {
            capacity: usize,
            write_batch_size: usize,
            read_batch_size: usize,
        }

        fn implementation(
            Opts {
                capacity,
                write_batch_size,
                read_batch_size,
            }: Opts,
        ) {
            const TOTAL: u64 = 100000;
            let (mut reader, mut writer) = create_ring::<u64>(capacity);

            let bg_thread = spawn(move || {
                let mut iterator = 0..TOTAL;

                loop {
                    let mut batch = Vec::with_capacity(write_batch_size);

                    for i in &mut iterator {
                        batch.push(i);
                        if batch.len() == write_batch_size {
                            break;
                        }
                    }

                    let mut done = 0usize;
                    loop {
                        if done == batch.len() {
                            break;
                        }
                        done += writer.write_from_slice(&batch[done..]);
                    }

                    if batch.len() != write_batch_size {
                        break;
                    }
                }
            });

            let iterator = std::iter::from_fn(move || {
                let mut destination = vec![0u64; read_batch_size];
                let mut got = 0;
                while reader.has_writer() {
                    got = reader.read_to_slice(&mut destination[..]);
                    if got != 0 {
                        break;
                    }
                }

                if got == 0 {
                    None
                } else {
                    Some(destination.into_iter().take(got))
                }
            })
            .flatten();

            for (got, expected) in iterator.zip(0..TOTAL) {
                assert_eq!(got, expected);
            }

            bg_thread.join().unwrap();
        }

        implementation(Opts {
            capacity: 100,
            read_batch_size: 6,
            write_batch_size: 3,
        });
        implementation(Opts {
            capacity: 100,
            read_batch_size: 3,
            write_batch_size: 6,
        });
        implementation(Opts {
            capacity: 10000,
            read_batch_size: 101,
            write_batch_size: 123,
        });
    }
}

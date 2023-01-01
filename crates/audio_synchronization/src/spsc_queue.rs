//! An unbounded SPSC queue.
//!
//! this queue consists of segments which are allocated on demand, then never deallocated. Instead, they are added to a
//! freelist and reused.
use std::alloc::Layout;
use std::cell::UnsafeCell;
use std::num::NonZeroUsize;
use std::ptr::{null_mut, NonNull};

use crate::sync::{AtomicPtr, AtomicU64, AtomicUsize, Ordering};

// The implementation works as follows:
//
// - New items are detected by incrementing a writer index and comparing with a reader index.
// - the offset in a chunk is determined by looking at the reader index where the chunk was created and subtracting it
//   from the needed element, iterating forward chunk by chunk until a chunk with the needed item is found.
// - Chunks are allocated on demand. Unused chunks are stored in a freelist.
// - Each chunk consists of a header followed by a body, where we store the pointer to the headers and get the pointer
//   to the bodies through pointer arithmetic.  This allows using `AtomicPtr` since the size is part of the data, not
//   part of the pointer to the data.
// - The SPSC part of this is guaranteed by Rust's type system: the reader and writer halves don't impl Sync or Clone.
// - There is a trivial destructor requirement in practice. Requiring Copy prevents the user from giving us something
//   with Drop on it, so we can leak objects and simplify the implementation: rather than fiddling with `MaybeUninit`,
//   all memory is guaranteed to be written to before it is read.
// - Zero-sized types never allocate a block at all and just move counters around.

#[derive(Clone)]
struct QueuePtrWrapped<T: Copy> {
    inner: NonNull<SpscQueue<T>>,
}

/// A sender for a SPSC queue.
///
/// use `.send()` to send a value to the queue.
pub struct SpscSender<T: Copy> {
    queue: QueuePtrWrapped<T>,
}

/// A receiver for a SPSC queue.
///
/// Use `.recv()` to get values.
pub struct SpscReceiver<T: Copy> {
    queue: QueuePtrWrapped<T>,
}

/// A chunk of entries consists of a header and then some number of allocated, possibly uninitialized entries after that
/// header.
///
/// All fields here are owned by the producer, save for the `next_chunk` field which will be used by the consumer when
/// it wishes to link chunks into the freelist.
struct ChunkHeader<T> {
    /// The first item in this chunk started at this writer index.
    first_index: u64,

    /// The chunk has space for this many elements.
    capacity: NonZeroUsize,

    /// The next chunk, either in the freelist or in the queue order.
    ///
    /// We don't use separate fields for these so that we can reduce the size.
    next_chunk: AtomicPtr<ChunkHeader<T>>,
}

fn chunk_layout<T: Copy>(capacity: NonZeroUsize) -> Layout {
    assert_ne!(std::mem::size_of::<T>(), 0);

    let chunk_layout = Layout::new::<ChunkHeader<T>>();
    let arr_layout = Layout::array::<T>(capacity.get()).unwrap();
    let (full_layout, _) = chunk_layout.extend(arr_layout).unwrap();
    full_layout.pad_to_align()
}

/// Allocate a chunk for `capacity` elements, then return a pointer to the header for that chunk.
fn new_chunk<T: Copy>(first_index: u64, capacity: NonZeroUsize) -> NonNull<ChunkHeader<T>> {
    let full_layout = chunk_layout::<T>(capacity);
    unsafe {
        let dest_ptr = std::alloc::alloc(full_layout);
        let h_ptr = dest_ptr as *mut ChunkHeader<T>;
        h_ptr.write(ChunkHeader {
            capacity,
            first_index,
            next_chunk: AtomicPtr::new(null_mut()),
        });
        NonNull::new_unchecked(h_ptr)
    }
}

/// get the data pointer for a chunk.
fn get_data_ptr<T: Copy>(chunk_ptr: NonNull<ChunkHeader<T>>) -> NonNull<T> {
    // First, go to an i8.
    let mut as_i8 = chunk_ptr.as_ptr() as *mut i8;
    unsafe {
        as_i8 = as_i8.add(std::mem::size_of::<ChunkHeader<T>>());
    }
    let offset = as_i8.align_offset(std::mem::align_of::<T>());
    // Rust docs say this can happen but are cagey about when, then go on to show examples that ignores this case.
    // Let's crash if it comes up.
    assert_ne!(offset, usize::MAX);
    unsafe { NonNull::new_unchecked(as_i8.add(offset).cast()) }
}

/// State for the consumer side of the queue.
struct ConsumerState<T: Copy> {
    /// The last index which was read by the consumer, plus 1.
    next_read: u64,

    /// The current chunk the consumer is consuming from.
    cur_chunk: NonNull<ChunkHeader<T>>,
}

/// State for the producer side of the queue.
struct ProducerState<T: Copy> {
    /// The current chunk the producer is writing to.
    cur_chunk: NonNull<ChunkHeader<T>>,
}

struct SpscQueue<T: Copy> {
    /// Starts at 2, one for the sender and one for the receiver, and decrements when they're dropped.
    refcount: AtomicUsize,

    producer_state: UnsafeCell<ProducerState<T>>,

    consumer_state: UnsafeCell<ConsumerState<T>>,

    freelist_head: AtomicPtr<ChunkHeader<T>>,

    /// The producer's index, which is shared state because it is used by the consumer to know when an item is available.
    producer_index: AtomicU64,

    /// the size of each chunk in this queue.
    chunk_size: NonZeroUsize,
}

impl<T: Copy> SpscQueue<T> {
    fn new(chunk_size: NonZeroUsize) -> Self {
        let first_chunk = if std::mem::size_of::<T>() == 0 {
            NonNull::dangling()
        } else {
            new_chunk(0, chunk_size)
        };

        SpscQueue {
            refcount: AtomicUsize::new(2),
            producer_state: UnsafeCell::new(ProducerState {
                cur_chunk: first_chunk,
            }),
            consumer_state: UnsafeCell::new(ConsumerState {
                cur_chunk: first_chunk,
                next_read: 0,
            }),
            chunk_size,
            freelist_head: AtomicPtr::new(null_mut()),
            producer_index: AtomicU64::new(0),
        }
    }

    /// # Safety
    ///
    /// Must only ever be called from one thread at a time.
    unsafe fn dequeue(&self) -> Option<T> {
        let cstate = &mut *self.consumer_state.get();
        let producer_index_snapshot = self.producer_index.load(Ordering::Acquire);
        // has the producer index moved past the one we want to read?
        let has_one = producer_index_snapshot > cstate.next_read;
        if !has_one {
            return None;
        }

        if std::mem::size_of::<T>() == 0 {
            cstate.next_read += 1;
            return Some(NonNull::<T>::dangling().as_ptr().read());
        }

        for _ in 0..2 {
            let chunk_start = cstate.cur_chunk.as_ref().first_index;
            let chunk_end = chunk_start + cstate.cur_chunk.as_ref().capacity.get() as u64;
            if (chunk_start..chunk_end).contains(&cstate.next_read) {
                let dptr = get_data_ptr(cstate.cur_chunk);
                let out = dptr
                    .as_ptr()
                    .add((cstate.next_read - chunk_start).try_into().unwrap())
                    .read();
                cstate.next_read += 1;
                return Some(out);
            }

            let new_next = cstate.cur_chunk.as_ref().next_chunk.load(Ordering::Relaxed);
            let old = cstate.cur_chunk;
            // The producer will always publish new chunks before incrementing the index, ergo this should always be non-NULL.
            cstate.cur_chunk = NonNull::new(new_next).expect(
                "The producer failed to publish a chunk before claiming there was new data",
            );

            // Now we must put old onto the head of the freelist.
            let mut old_head = self.freelist_head.load(Ordering::Relaxed);
            loop {
                old.as_ref().next_chunk.store(old_head, Ordering::Relaxed);
                match self.freelist_head.compare_exchange(
                    old_head,
                    old.as_ptr(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(o) => old_head = o,
                }
            }
        }

        panic!("Logic error: it took more than 2 chunks to find an element to dequeue, but chunks always contain at least one element");
    }

    /// # Safety
    ///
    /// Must only be called from one thread at a time.
    unsafe fn enqueue(&self, value: T) {
        if std::mem::size_of::<T>() == 0 {
            self.producer_index.fetch_add(1, Ordering::Release);
            return;
        }

        let pstate = self.producer_state.get().as_mut().unwrap();

        let prod_index = self.producer_index.load(Ordering::Relaxed);
        let cur_chunk = pstate.cur_chunk;
        let offset = (prod_index - cur_chunk.as_ref().first_index) as usize;
        if offset < cur_chunk.as_ref().capacity.get() {
            // Easy: just put it in and return.
            let dptr = get_data_ptr(cur_chunk);
            dptr.as_ptr().add(offset as usize).write(value);
            self.producer_index.fetch_add(1, Ordering::Release);
            return;
        }

        let new_chunk = self.find_or_alloc_chunk(prod_index);
        let dptr = get_data_ptr(new_chunk);
        dptr.as_ptr().write(value);
        pstate.cur_chunk = new_chunk;
        cur_chunk
            .as_ref()
            .next_chunk
            .store(new_chunk.as_ptr(), Ordering::Relaxed);
        self.producer_index.fetch_add(1, Ordering::Release);
    }

    unsafe fn find_or_alloc_chunk(&self, first_index: u64) -> NonNull<ChunkHeader<T>> {
        let mut freelist_head = self.freelist_head.load(Ordering::Relaxed);
        while !freelist_head.is_null() {
            let next = freelist_head
                .as_ref()
                .unwrap()
                .next_chunk
                .load(Ordering::Relaxed);
            match self.freelist_head.compare_exchange(
                freelist_head,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    freelist_head
                        .as_ref()
                        .unwrap()
                        .next_chunk
                        .store(null_mut(), Ordering::Relaxed);
                    freelist_head.as_mut().unwrap().first_index = first_index;
                    return NonNull::new(freelist_head).unwrap();
                }
                Err(h) => freelist_head = h,
            }
        }

        new_chunk(first_index, self.chunk_size)
    }
}

impl<T: Copy> Drop for SpscQueue<T> {
    fn drop(&mut self) {
        let mut freelist_head = self.freelist_head.load(Ordering::Relaxed);
        while !freelist_head.is_null() {
            let next = unsafe {
                freelist_head
                    .as_ref()
                    .unwrap()
                    .next_chunk
                    .load(Ordering::Relaxed)
            };
            unsafe {
                std::alloc::dealloc(freelist_head.cast(), chunk_layout::<T>(self.chunk_size));
            }
            freelist_head = next;
        }

        let mut queue_head = unsafe {
            self.consumer_state
                .get()
                .as_ref()
                .unwrap()
                .cur_chunk
                .as_ptr()
        };
        while !queue_head.is_null() {
            let next = unsafe {
                queue_head
                    .as_ref()
                    .unwrap()
                    .next_chunk
                    .load(Ordering::Relaxed)
            };
            unsafe {
                std::alloc::dealloc(queue_head.cast(), chunk_layout::<T>(self.chunk_size));
            }
            queue_head = next;
        }
    }
}

fn queue_layout<T: Copy>() -> Layout {
    Layout::new::<SpscQueue<T>>()
}

unsafe fn alloc_queue<T: Copy>(chunk_size: NonZeroUsize) -> QueuePtrWrapped<T> {
    let l = queue_layout::<T>();
    let ptr = std::alloc::alloc(l).cast::<SpscQueue<T>>();
    let ptr = NonNull::new(ptr).unwrap();
    ptr.as_ptr().write(SpscQueue::new(chunk_size));
    QueuePtrWrapped { inner: ptr }
}

unsafe fn dealloc_queue<T: Copy>(queue: NonNull<SpscQueue<T>>) {
    let l = queue_layout::<T>();
    std::alloc::dealloc(queue.as_ptr().cast(), l);
}

impl<T: Copy> Drop for QueuePtrWrapped<T> {
    fn drop(&mut self) {
        unsafe {
            let old_refcount = self.inner.as_ref().refcount.fetch_sub(1, Ordering::Acquire);
            if old_refcount - 1 == 0 {
                dealloc_queue(self.inner);
            }
        }
    }
}

/// Construct a queue which will allocate chunks of memory of `chunk_size` elements.
pub fn spsc_queue<T: Copy>(chunk_size: NonZeroUsize) -> (SpscSender<T>, SpscReceiver<T>) {
    let queue = unsafe { alloc_queue::<T>(chunk_size) };
    (
        SpscSender {
            queue: queue.clone(),
        },
        SpscReceiver { queue },
    )
}

impl<T: Copy> SpscSender<T> {
    /// Enqueue an item into this queue.
    ///
    /// Allocates roughly whenever there are `chunk_size` outstanding items in the queue until the queue reaches a steady state where the throughput of the receiver matches the sender.
    pub fn send(&mut self, val: T) {
        unsafe {
            self.queue.inner.as_ref().enqueue(val);
        }
    }
}

impl<T: Copy> SpscReceiver<T> {
    /// Receive an item from the queue if one is present.
    ///
    /// This function is lockfree and does not deallocate.
    pub fn recv(&mut self) -> Option<T> {
        unsafe { self.queue.inner.as_ref().dequeue() }
    }
}

unsafe impl<T: Copy> Send for SpscSender<T> {}
unsafe impl<T: Copy> Send for SpscReceiver<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
    struct Counter(usize);

    #[test]
    fn test_simple() {
        crate::sync::wrap_test(test_simple_inner);
    }

    fn test_simple_inner() {
        let (mut sender, mut receiver) = spsc_queue::<Counter>(NonZeroUsize::new(3).unwrap());

        let sender_thread = crate::sync::spawn(move || {
            for i in 0..10 {
                sender.send(Counter(i));
            }
        });

        let receiver_thread = crate::sync::spawn(move || {
            let mut out = vec![];
            while out.len() < 10 {
                if let Some(r) = receiver.recv() {
                    out.push(r.0);
                } else {
                    crate::sync::yield_now();
                }
            }

            out
        });

        sender_thread.join().unwrap();
        let got = receiver_thread.join().unwrap();
        assert_eq!(got, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}

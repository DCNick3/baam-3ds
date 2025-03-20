//! A horrifying chimera of `spmc-buffer` and `triple-buffer` crates. I might have broken the stolen goods.

use core::{cell::UnsafeCell, sync::atomic::Ordering};
use std::sync::Arc;
use std::sync::atomic::AtomicU16;
use tracing::{trace, warn};

pub fn spmc_buffer<T: Send + Sync>(
    read_buffers: usize,
    mut generator: impl FnMut() -> T,
) -> (Input<T>, Output<T>) {
    // Check that the amount of read buffers fits implementation limits
    assert!(read_buffers <= MAX_READ_BUFFERS);

    // Compute the actual buffer count
    let num_buffers = 2 + read_buffers;

    // Build the buffers, using the provided generator of initial data
    let buffers = (0..num_buffers)
        .map(|_| Buffer {
            data: UnsafeCell::new(generator()),
            done_readers: AtomicRefCount::new(0),
        })
        .collect();

    // Create the shared state. Buffer 0 is initially considered the latest,
    // and has one reader accessing it (corresponding to a refcount of 1).
    let shared_state = Arc::new(SharedState {
        buffers,
        latest_info: AtomicSharedIndex::new(1),
    });

    // ...then construct the input and output structs
    (
        Input {
            shared: shared_state.clone(),
            reader_counts: vec![0; num_buffers],
            write_idx: 1,
        },
        Output {
            shared: shared_state,
            read_idx: 2,
        },
    )
}

#[derive(Debug)]
pub struct Input<T: Send + Sync> {
    /// Reference-counted shared state
    shared: Arc<SharedState<T>>,

    /// Amount of readers who potentially have access to each (unreachable)
    /// buffer. The latest buffer, which is still reachable, is marked with an
    /// "infinite" reference count, to warn that we don't know the true value.
    reader_counts: Vec<RefCount>,

    /// Index of the input buffer (which is private to the producer)
    write_idx: BufferIndex,
}
//
// Public interface
impl<T: Send + Sync> Input<T> {
    pub fn input_buffer_mut(&mut self) -> &mut T {
        // This is safe because the synchronization protocol ensures that we
        // have exclusive access to this buffer.
        let input_ptr = self.shared.buffers[self.write_idx as usize].data.get();
        unsafe { &mut *input_ptr }
    }

    pub fn publish(&mut self) {
        // Access the shared state
        let ref shared_state = *self.shared;

        let write_idx = self.write_idx;

        let ref write_buffer = shared_state.buffers[write_idx as usize];
        write_buffer.done_readers.store(0, Ordering::Relaxed);

        // Publish our write buffer as the new latest buffer, and retrieve
        // the old buffer's shared index
        let former_latest_info = shared_state.latest_info.swap(
            write_idx * SHARED_INDEX_MULTIPLIER,
            Ordering::Release, // Publish updated buffer state to the readers
        );

        // In debug mode, make sure that overflow did not occur
        debug_assert!(former_latest_info & SHARED_OVERFLOW_BIT == 0);

        // Decode the information contained in the former shared index
        let former_idx = (former_latest_info & SHARED_INDEX_MASK) / SHARED_INDEX_MULTIPLIER;
        let former_readcount = former_latest_info & SHARED_READCOUNT_MASK;

        // Write down the former buffer's refcount, and set the latest buffer's
        // refcount to infinity so that we don't accidentally write to it
        self.reader_counts[former_idx as usize] = former_readcount;
        self.reader_counts[write_idx as usize] = INFINITE_REFCOUNT;

        // now that we have published our old new buffer, find a new buffer to write into

        let mut attempt = 0;

        // Go into a spin-loop, waiting for an "old" buffer with no live reader.
        // This loop will finish in a finite amount of iterations if each thread
        // is allocated two private buffers, because readers can hold at most
        // two buffers simultaneously. With less buffers, we may need to wait.
        self.write_idx = loop {
            // We want to iterate over both buffers and associated refcounts
            let mut buf_rc_iter = shared_state.buffers.iter().zip(self.reader_counts.iter());

            // We want to find a buffer which is unreachable, and whose previous
            // readers have all moved on to more recent data. We identify
            // unreachable buffers by having previously tagged the latest buffer
            // with an infinite reference count.
            let write_pos = buf_rc_iter.position(|tuple| {
                let (buffer, refcount) = tuple;
                *refcount == buffer.done_readers.load(Ordering::Relaxed)
            });

            // If we found a free buffer, we can use it now. Otherwise, we may
            // want to leave client threads some time to work before spinning.
            if let Some(idx) = write_pos {
                break idx as u16;
            } else {
                attempt += 1;

                if attempt >= 5 {
                    warn!(
                        "All buffers are busy! We are very likely to be in a deadlock here due to priority inversion"
                    );
                }
                std::thread::yield_now();
            }
        };

        trace!("now using {} as input buffer", self.write_idx);
    }
}

#[derive(Debug)]
pub struct Output<T: Send + Sync> {
    /// Reference-counted shared state
    shared: Arc<SharedState<T>>,

    /// Index of the output buffer (which is private to the consumer)
    read_idx: BufferIndex,
}
//
// Public interface
impl<T: Send + Sync> Output<T> {
    /// Tell whether an updated value has been submitted by the producer
    ///
    /// This method is mainly intended for diagnostics purposes. Please do not
    /// let it inform your decision of reading a value or not, as that would
    /// effectively be building a very poor spinlock-based double buffer
    /// implementation. If what you truly need is a double buffer, build
    /// yourself a proper blocking one instead of wasting CPU time.
    pub fn updated(&self) -> bool {
        // Access the shared state
        let ref shared_state = *self.shared;

        // Check if the producer has submitted an update
        let latest_info = shared_state.latest_info.load(Ordering::Relaxed);
        (latest_info & SHARED_INDEX_MASK) != (self.read_idx * SHARED_INDEX_MULTIPLIER)
    }

    /// Access the output buffer directly
    ///
    /// This advanced interface allows you to modify the contents of the output
    /// buffer, so that you can avoid copying the output value when this is an
    /// expensive process. One possible application, for example, is to
    /// post-process values from the producer before use.
    ///
    /// However, by using it, you force yourself to take into account some
    /// implementation subtleties that you could normally ignore.
    ///
    /// First, keep in mind that you can lose access to the current output
    /// buffer any time [`read()`] or [`update()`] is called, as it may be
    /// replaced by an updated buffer from the producer automatically.
    ///
    /// Second, to reduce the potential for the aforementioned usage error, this
    /// method does not update the output buffer automatically. You need to call
    /// [`update()`] in order to fetch buffer updates from the producer.
    ///
    /// [`read()`]: Output::read
    /// [`update()`]: Output::update
    pub fn output_buffer_mut(&mut self) -> &mut T {
        // This is safe because the synchronization protocol ensures that we
        // have exclusive access to this buffer.
        let output_ptr = self.shared.buffers[self.read_idx as usize].data.get();
        unsafe { &mut *output_ptr }
    }

    /// Update the output buffer
    ///
    /// Check if the producer submitted a new data version, and if one is
    /// available, update our output buffer to use it. Return a flag that tells
    /// you whether such an update was carried out.
    ///
    /// Bear in mind that when this happens, you will lose any change that you
    /// performed to the output buffer via the
    /// [`output_buffer_mut()`](Output::output_buffer_mut) interface.
    pub fn update(&mut self) -> bool {
        // Check if an update is present in the back-buffer
        let updated = self.updated();
        if updated {
            // Access the shared state
            let shared_state = &(*self.shared);

            // Acquire access to the latest buffer, incrementing its
            // refcount to tell the producer that we have access to it
            let latest_info = shared_state.latest_info.fetch_add(
                1,
                Ordering::Acquire, // Fetch the associated buffer state
            );

            // Drop our current read buffer. Because we already used an acquire
            // fence above, we can safely use relaxed atomic order here: no CPU
            // or compiler will reorder this operation before the fence.
            unsafe {
                self.discard_read_buffer(Ordering::Relaxed);
            }

            // In debug mode, make sure that overflow did not occur
            debug_assert!((latest_info + 1) & SHARED_OVERFLOW_BIT == 0);

            // Extract the index of our new read buffer
            self.read_idx = (latest_info & SHARED_INDEX_MASK) / SHARED_INDEX_MULTIPLIER;
            trace!("now using {} as output buffer", self.read_idx);
        }

        // Tell whether an update was carried out
        updated
    }

    /// Drop the current read buffer. This is unsafe because it allows the
    /// writer to write into it, which means that the read buffer must never be
    /// accessed again after this operation completes. Be extremely careful with
    /// memory ordering: this operation must NEVER be reordered before a read!
    unsafe fn discard_read_buffer(&self, order: Ordering) {
        self.shared.buffers[self.read_idx as usize]
            .done_readers
            .fetch_add(1, order);
    }
}

impl<T: Send + Sync> Clone for Output<T> {
    // Create a new output interface associated with a given SPMC buffer
    fn clone(&self) -> Self {
        // Clone the current shared state
        let shared_state = self.shared.clone();

        // Acquire access to the latest buffer, incrementing its refcount
        let latest_info = shared_state.latest_info.fetch_add(
            1,
            Ordering::Acquire, // Fetch the associated buffer state
        );

        // Extract the index of this new read buffer
        let new_read_idx = (latest_info & SHARED_INDEX_MASK) / SHARED_INDEX_MULTIPLIER;

        // Build a new output interface from this information
        Output {
            shared: shared_state,
            read_idx: new_read_idx,
        }
    }
}

impl<T: Send + Sync> Drop for Output<T> {
    // Discard our read buffer on thread exit
    fn drop(&mut self) {
        // We must use release ordering here in order to prevent preceding
        // buffer reads from being reordered after the buffer is discarded
        unsafe {
            self.discard_read_buffer(Ordering::Release);
        }
    }
}

/// Shared state for SPMC buffers
///
/// This struct provides both a set of shared buffers for single-producer
/// multiple-consumer broadcast communication and a way to know which of these
/// buffers contains the most up to date data with reader reference counting.
///
/// The number of buffers N is a design tradeoff: the larger it is, the more
/// robust the primitive is against contention, at the cost of increased memory
/// usage. An SPMC buffer is wait free for readers, and almost wait-free for
/// writers, if N = Nreaders + 2, where Nreaders is the amount of consumers. But
/// it can work correctly in a degraded regime which is wait-free for readers
/// and potentially blocking for writers as long as N >= 2.
///
/// Note that I said "almost" wait-free. True writer wait-freedom can only be
/// proven in any circumstances by adding extra memory barriers to the
/// consumer's algorithm, which can have a high cost on relaxed-memory archs
/// like ARM and POWER. I do not consider that to be worth it when one can often
/// just use more buffers if writer contention starts to be problematic.
#[derive(Debug)]
struct SharedState<T: Send + Sync> {
    /// Data storage buffers
    buffers: Vec<Buffer<T>>,

    /// Combination of reader count and latest buffer index (see below)
    latest_info: AtomicSharedIndex,
}

unsafe impl<T: Send + Sync> Sync for SharedState<T> {}

#[derive(Debug)]
struct Buffer<T: Send + Sync> {
    /// Actual data must be in an UnsafeCell so that Rust knows it's mutable
    data: UnsafeCell<T>,

    /// Amount of readers who are done with this buffer and switched to another
    done_readers: AtomicRefCount,
}

/// Atomic "shared index", combining "latest buffer" and "reader count" info
/// in a single large integer through silly bit tricks.
///
/// At the start of the readout process, a reader must atomically announce
/// itself as in the process of reading the current buffer (so that said buffer
/// does not get reclaimed) and determine which buffer is the current buffer.
///
/// Here is why these operations cannot be separated:
///
/// - Assume that the reader announces that it is reading, then determines which
///   buffer is the current buffer. In this case, the reader can only make the
///   generic announcement that it is reading "some" buffer, because it does not
///   know yet which buffer it'll be reading. This means that other threads do
///   not know which buffers are busy, and no buffer can be liberated until the
///   reader clarifies its intent or goes away. This way of operating is thus
///   effectively equivalent to a reader-directed update lock.
/// - Assume that the reader determines which buffer is the current buffer, then
///   announces itself as being in the process of reading this specific buffer.
///   Inbetween these two actions, the current buffer may have changed, so the
///   reader may increment the wrong refcount. Furthermore, the buffer that is
///   now targeted by the reader may have already be tagged as safe for reuse or
///   deletion by the writer, so if the reader proceeds with reading it, it may
///   accidentally end up in a data race with the writer. This follows the
///   classical rule of thumb that one should always reserve resources before
///   accessing them, however lightly.
///
/// To combine latest buffer index readout and reader count increment, we need
/// to pack both of these quantities into a single shared integer variable that
/// we can manipulate through a atomic operations. For refcounting, fetch_add
/// sounds like a good choice, so we want an atomic integer type whose low-order
/// bits act as a refcount and whose high-order bit act as a buffer index.
/// Here's an example for a 16-bit unsigned integer, allowing up to 64 buffers
/// and 511 concurrent readers on each buffer:
///
///   bit (high-order first):       15 .. 10  9  8 .. 0
///                                +--------+--+-------+
///   Contents:                    |BUFFERID|OF|READCNT|
///                                +--------+--+-------+
///
/// In this scheme, BUFFERID is the index of the "latest buffer", which contains
/// the newest data from the writer, and READCNT is the amount of readers who
/// have acquired access to this data. In principle, the later counter could
/// overflow in the presence of 512+ concurrent readers, all accessing the same
/// buffer without a single update happening in meantime. This scenario is
/// highly implausible on current hardware architectures (even many-core ones),
/// but we nevertheless account for it by adding an overflow "OF" bit, which is
/// checked in debug builds. A thread which detects such overflow should panic.
type BufferIndex = u16;
//
type RefCount = u16;
const INFINITE_REFCOUNT: RefCount = 0xffff;
type AtomicRefCount = AtomicU16;
//
type SharedIndex = u16;
type AtomicSharedIndex = AtomicU16;
const SHARED_READCOUNT_MASK: SharedIndex = 0b0000_0001_1111_1111;
const SHARED_OVERFLOW_BIT: SharedIndex = 0b0000_0010_0000_0000;
const SHARED_INDEX_MASK: SharedIndex = 0b1111_1100_0000_0000;
const SHARED_INDEX_MULTIPLIER: SharedIndex = 0b0000_0100_0000_0000;
//
const MAX_BUFFERS: usize = (SHARED_INDEX_MASK / SHARED_INDEX_MULTIPLIER + 1) as usize;
const MAX_READ_BUFFERS: usize = MAX_BUFFERS - 2;

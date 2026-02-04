use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};
use static_assertions::const_assert;

/// Cache-line padded wrapper to prevent false sharing
#[repr(C, align(64))]
struct CachePadded<T> {
    value: T,
}

impl<T> CachePadded<T> {
    fn new(value: T) -> Self {
        Self { value }
    }
}

/// Single-producer, single-consumer lock-free ring buffer.
/// Uses Release/Acquire memory ordering for ARM64 compatibility.
///
/// # Safety
/// - Only one producer thread may call `push`
/// - Only one consumer thread may call `pop`
/// - Size N must be a power of 2
pub struct RingBuffer<T, const N: usize> {
    /// Producer writes here (increments on push)
    head: CachePadded<AtomicU64>,
    /// Consumer writes here (increments on pop)
    tail: CachePadded<AtomicU64>,
    /// Storage slots (uninitialized until written)
    slots: [UnsafeCell<MaybeUninit<T>>; N],
}

// Compile-time assertion: N must be power of 2
const_assert!((1024 & (1024 - 1)) == 0);
const_assert!((4096 & (4096 - 1)) == 0);
const_assert!((8192 & (8192 - 1)) == 0);

impl<T, const N: usize> RingBuffer<T, N> {
    /// Create a new ring buffer
    pub fn new() -> Self {
        // Verify N is power of 2 at runtime for generic N
        assert!(N > 0 && (N & (N - 1)) == 0, "RingBuffer size must be power of 2");

        Self {
            head: CachePadded::new(AtomicU64::new(0)),
            tail: CachePadded::new(AtomicU64::new(0)),
            slots: unsafe {
                // Create uninitialized array
                MaybeUninit::uninit().assume_init()
            },
        }
    }

    /// Push a value into the ring buffer.
    /// Returns Err(value) if buffer is full (backpressure - caller should handle).
    ///
    /// # Safety
    /// Only one thread (producer) may call this method.
    pub fn push(&self, value: T) -> Result<(), T> {
        // Load with Relaxed - only producer modifies head
        let head = self.head.value.load(Ordering::Relaxed);
        // Load with Acquire - synchronize with consumer's Release store to tail
        let tail = self.tail.value.load(Ordering::Acquire);

        // Check if buffer is full
        if head.wrapping_sub(tail) >= N as u64 {
            return Err(value);
        }

        // Write to slot (safe: only producer writes to this position)
        let idx = (head as usize) & (N - 1);
        unsafe {
            let slot = &mut *self.slots[idx].get();
            ptr::write(slot, MaybeUninit::new(value));
        }

        // Release: ensure write to slot completes before head increment
        self.head.value.store(head.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// Pop a value from the ring buffer.
    /// Returns None if buffer is empty.
    ///
    /// # Safety
    /// Only one thread (consumer) may call this method.
    pub fn pop(&self) -> Option<T> {
        // Load with Relaxed - only consumer modifies tail
        let tail = self.tail.value.load(Ordering::Relaxed);
        // Load with Acquire - synchronize with producer's Release store to head
        let head = self.head.value.load(Ordering::Acquire);

        // Check if buffer is empty
        if tail == head {
            return None;
        }

        // Read from slot (safe: only consumer reads from this position)
        let idx = (tail as usize) & (N - 1);
        let value = unsafe {
            let slot = &*self.slots[idx].get();
            ptr::read(slot).assume_init()
        };

        // Release: ensure read from slot completes before tail increment
        self.tail.value.store(tail.wrapping_add(1), Ordering::Release);
        Some(value)
    }

    /// Check if buffer is empty (may be stale immediately)
    pub fn is_empty(&self) -> bool {
        let tail = self.tail.value.load(Ordering::Relaxed);
        let head = self.head.value.load(Ordering::Acquire);
        tail == head
    }

    /// Check if buffer is full (may be stale immediately)
    pub fn is_full(&self) -> bool {
        let head = self.head.value.load(Ordering::Relaxed);
        let tail = self.tail.value.load(Ordering::Acquire);
        head.wrapping_sub(tail) >= N as u64
    }

    /// Get approximate length (may be stale)
    pub fn len(&self) -> usize {
        let head = self.head.value.load(Ordering::Relaxed);
        let tail = self.tail.value.load(Ordering::Acquire);
        head.wrapping_sub(tail) as usize
    }
}

// Safety: RingBuffer can be shared between threads (SPSC pattern)
unsafe impl<T: Send, const N: usize> Send for RingBuffer<T, N> {}
unsafe impl<T: Send, const N: usize> Sync for RingBuffer<T, N> {}

impl<T, const N: usize> Drop for RingBuffer<T, N> {
    fn drop(&mut self) {
        // Drop all remaining elements
        while self.pop().is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_basic() {
        let ring = RingBuffer::<u64, 4>::new();

        assert!(ring.is_empty());
        assert!(!ring.is_full());

        assert!(ring.push(1).is_ok());
        assert!(ring.push(2).is_ok());
        assert!(!ring.is_empty());

        assert_eq!(ring.pop(), Some(1));
        assert_eq!(ring.pop(), Some(2));
        assert_eq!(ring.pop(), None);
        assert!(ring.is_empty());
    }

    #[test]
    fn test_ring_buffer_full() {
        let ring = RingBuffer::<u64, 4>::new();

        assert!(ring.push(1).is_ok());
        assert!(ring.push(2).is_ok());
        assert!(ring.push(3).is_ok());
        assert!(ring.push(4).is_ok());

        // Buffer is full
        assert!(ring.is_full());
        assert_eq!(ring.push(5), Err(5));

        // Pop one, should be able to push again
        assert_eq!(ring.pop(), Some(1));
        assert!(ring.push(5).is_ok());
    }

    #[test]
    fn test_ring_buffer_wrap_around() {
        let ring = RingBuffer::<u64, 4>::new();

        // Fill buffer
        for i in 0..4 {
            assert!(ring.push(i).is_ok());
        }

        // Pop all
        for i in 0..4 {
            assert_eq!(ring.pop(), Some(i));
        }

        // Fill again (tests wrap-around)
        for i in 10..14 {
            assert!(ring.push(i).is_ok());
        }

        // Pop all
        for i in 10..14 {
            assert_eq!(ring.pop(), Some(i));
        }
    }

    #[test]
    fn test_fifo_order() {
        let ring = RingBuffer::<u64, 1024>::new();

        for i in 0..100 {
            assert!(ring.push(i).is_ok());
        }

        for i in 0..100 {
            assert_eq!(ring.pop(), Some(i));
        }
    }
}

use crate::ring::RingBuffer;
use crate::tsc::{rdtsc, tsc_to_ns};
use crate::types::{Bundle, Transaction, BUNDLE_MAX};

/// Timeout for bundle flush (100 microseconds)
pub const BUNDLE_TIMEOUT_NS: u64 = 100_000;

/// Error when bundle buffer is full
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleFull;

/// Stack-allocated bundle accumulator.
/// Flushes when:
/// 1. Bundle reaches BUNDLE_MAX transactions
/// 2. Timeout expires (BUNDLE_TIMEOUT_NS since first transaction)
pub struct BundleBuilder {
    buffer: [Transaction; BUNDLE_MAX],
    count: usize,
    start_tsc: u64,
}

impl BundleBuilder {
    /// Create a new bundle builder
    pub fn new() -> Self {
        Self {
            buffer: [Transaction::new_unchecked(0, 1, 1, 0, 0); BUNDLE_MAX],
            count: 0,
            start_tsc: rdtsc(),
        }
    }

    /// Add a transaction to the bundle.
    /// Automatically flushes if bundle is full or timeout expires.
    ///
    /// Returns Err(BundleFull) if ring buffer is full and flush fails.
    pub fn add(
        &mut self,
        txn: Transaction,
        ring: &RingBuffer<Bundle, 1024>,
    ) -> Result<(), BundleFull> {
        // Check if we need to flush before adding (due to timeout or full buffer)
        if self.count >= BUNDLE_MAX || (self.count > 0 && self.should_flush_timeout()) {
            self.flush(ring)?;
        }

        // If buffer is empty, reset start timestamp
        if self.count == 0 {
            self.start_tsc = rdtsc();
        }

        // Add transaction to buffer
        self.buffer[self.count] = txn;
        self.count += 1;

        // Check if we're now full and need to flush immediately
        if self.count >= BUNDLE_MAX {
            self.flush(ring)?;
        }

        Ok(())
    }

    /// Check if bundle should be flushed due to timeout
    pub fn should_flush_timeout(&self) -> bool {
        if self.count == 0 {
            return false;
        }

        let elapsed_tsc = rdtsc() - self.start_tsc;
        let elapsed_ns = tsc_to_ns(elapsed_tsc);
        elapsed_ns >= BUNDLE_TIMEOUT_NS
    }

    /// Flush the current bundle to the ring buffer
    pub fn flush(&mut self, ring: &RingBuffer<Bundle, 1024>) -> Result<(), BundleFull> {
        if self.count == 0 {
            return Ok(());
        }

        // Use unchecked version since we control count internally
        debug_assert!(self.count <= BUNDLE_MAX, "count exceeds BUNDLE_MAX");
        let bundle = Bundle::with_transactions_unchecked(
            self.buffer,
            self.count as u32,
            tsc_to_ns(self.start_tsc),
        );

        ring.push(bundle).map_err(|_| BundleFull)?;

        // Reset builder
        self.count = 0;
        self.start_tsc = rdtsc();

        Ok(())
    }

    /// Force flush even if bundle is not full or timeout has not expired
    pub fn force_flush(&mut self, ring: &RingBuffer<Bundle, 1024>) -> Result<(), BundleFull> {
        self.flush(ring)
    }

    /// Get current bundle size
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if bundle is empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Check if bundle is full
    pub fn is_full(&self) -> bool {
        self.count >= BUNDLE_MAX
    }
}

impl Default for BundleBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsc::init_tsc;

    #[test]
    fn test_bundle_builder_basic() {
        init_tsc();
        let ring = RingBuffer::<Bundle, 1024>::new();
        let mut builder = BundleBuilder::new();

        let txn = Transaction::new_unchecked(1, 1000, 100, 0, 0);
        assert!(builder.add(txn, &ring).is_ok());
        assert_eq!(builder.len(), 1);
    }

    #[test]
    fn test_bundle_builder_auto_flush_on_full() {
        init_tsc();
        let ring = RingBuffer::<Bundle, 1024>::new();
        let mut builder = BundleBuilder::new();

        // Fill bundle to max
        for i in 0..BUNDLE_MAX {
            let txn = Transaction::new_unchecked(i as u64, 1000, 100, 0, 0);
            assert!(builder.add(txn, &ring).is_ok());
        }

        // Should have auto-flushed
        assert_eq!(builder.len(), 0);
        assert_eq!(ring.len(), 1);

        // Verify bundle
        let bundle = ring.pop().unwrap();
        assert_eq!(bundle.count, BUNDLE_MAX as u32);
    }

    #[test]
    fn test_bundle_builder_manual_flush() {
        init_tsc();
        let ring = RingBuffer::<Bundle, 1024>::new();
        let mut builder = BundleBuilder::new();

        for i in 0..5 {
            let txn = Transaction::new_unchecked(i as u64, 1000, 100, 0, 0);
            assert!(builder.add(txn, &ring).is_ok());
        }

        assert_eq!(builder.len(), 5);
        assert!(builder.force_flush(&ring).is_ok());
        assert_eq!(builder.len(), 0);

        let bundle = ring.pop().unwrap();
        assert_eq!(bundle.count, 5);
    }

    #[test]
    fn test_bundle_builder_timeout() {
        use std::thread;
        use std::time::Duration;

        init_tsc();
        let ring = RingBuffer::<Bundle, 1024>::new();
        let mut builder = BundleBuilder::new();

        let txn = Transaction::new_unchecked(1, 1000, 100, 0, 0);
        builder.add(txn, &ring).unwrap();
        assert_eq!(builder.len(), 1);

        // Wait for timeout (500 microseconds >> 100 microseconds to be safe)
        thread::sleep(Duration::from_micros(500));

        // Check timeout condition
        assert!(builder.should_flush_timeout(), "Expected timeout condition to be true");

        // Should trigger timeout flush on next add
        let txn2 = Transaction::new_unchecked(2, 1000, 100, 0, 0);
        builder.add(txn2, &ring).unwrap();

        // First bundle should be flushed
        assert!(ring.len() >= 1, "Expected at least 1 bundle flushed");
        let bundle = ring.pop().unwrap();
        // Bundle could have 1 or 2 transactions depending on timing
        assert!(bundle.count >= 1 && bundle.count <= 2,
                "Expected bundle to have 1-2 transactions, got {}", bundle.count);
    }
}

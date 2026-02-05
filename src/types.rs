use core::fmt;
use core::ptr;
use static_assertions::{const_assert, const_assert_eq};
use crate::errors::{TransactionError, BundleError};

/// Fixed bundle size for compile-time allocation
pub const BUNDLE_MAX: usize = 16;

/// Transaction represents a single order on the order book.
/// Zero-heap, repr(C) for cache predictability.
#[repr(C)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub id: u64,
    pub price: i64,        // Fixed-point: divide by 10000 for decimal (4 places)
    pub size: u32,
    pub side: u8,          // 0=bid, 1=ask
    _padding1: [u8; 3],    // Align to 8 bytes
    pub ingress_ts_ns: u64,
}

// Compile-time assertions for Transaction layout
const_assert_eq!(core::mem::size_of::<Transaction>(), 32);
const_assert_eq!(core::mem::align_of::<Transaction>(), 8);

impl Transaction {
    /// Create a new transaction with validation
    ///
    /// # Errors
    /// - `InvalidSide`: if side is not 0 (bid) or 1 (ask)
    /// - `NegativePrice`: if price is <= 0
    /// - `ZeroSize`: if size is 0
    pub fn new(id: u64, price: i64, size: u32, side: u8, ingress_ts_ns: u64) -> Result<Self, TransactionError> {
        // Validate side (must be 0 or 1)
        if side > 1 {
            return Err(TransactionError::InvalidSide(side));
        }

        // Validate price (must be positive)
        if price <= 0 {
            return Err(TransactionError::NegativePrice(price));
        }

        // Validate size (must be non-zero)
        if size == 0 {
            return Err(TransactionError::ZeroSize);
        }

        Ok(Self {
            id,
            price,
            size,
            side,
            _padding1: [0; 3],
            ingress_ts_ns,
        })
    }

    /// Create a new transaction without validation (for testing/trusted inputs)
    ///
    /// # Safety
    /// Caller must ensure:
    /// - side is 0 or 1
    /// - price is positive
    /// - size is non-zero
    pub fn new_unchecked(id: u64, price: i64, size: u32, side: u8, ingress_ts_ns: u64) -> Self {
        debug_assert!(side <= 1, "side must be 0 or 1");
        debug_assert!(price > 0, "price must be positive");
        debug_assert!(size > 0, "size must be non-zero");

        Self {
            id,
            price,
            size,
            side,
            _padding1: [0; 3],
            ingress_ts_ns,
        }
    }

    /// Zero-copy serialization to bytes
    pub fn to_bytes(&self) -> [u8; 32] {
        unsafe {
            let mut bytes = [0u8; 32];
            ptr::copy_nonoverlapping(
                self as *const Self as *const u8,
                bytes.as_mut_ptr(),
                32,
            );
            bytes
        }
    }

    /// Zero-copy deserialization from bytes
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        unsafe {
            let mut txn = core::mem::MaybeUninit::<Transaction>::uninit();
            ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                txn.as_mut_ptr() as *mut u8,
                32,
            );
            txn.assume_init()
        }
    }

    /// Get price as f64 for display
    pub fn price_f64(&self) -> f64 {
        self.price as f64 / 10000.0
    }

    /// Check if transaction is a bid
    pub fn is_bid(&self) -> bool {
        self.side == 0
    }

    /// Check if transaction is an ask
    pub fn is_ask(&self) -> bool {
        self.side == 1
    }
}

impl fmt::Debug for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Transaction")
            .field("id", &self.id)
            .field("price", &self.price_f64())
            .field("size", &self.size)
            .field("side", &if self.is_bid() { "BID" } else { "ASK" })
            .field("ingress_ts_ns", &self.ingress_ts_ns)
            .finish()
    }
}

/// Bundle represents a batch of transactions ready for submission.
/// Stack-allocated, zero-heap.
#[repr(C)]
#[derive(Copy, Clone, PartialEq)]
pub struct Bundle {
    pub transactions: [Transaction; BUNDLE_MAX],
    pub count: u32,
    _padding: u32,  // Align to 8 bytes
    pub timestamp_ns: u64,
}

// Compile-time assertion for Bundle
const_assert!(core::mem::size_of::<Bundle>() == 32 * BUNDLE_MAX + 16);

impl Bundle {
    /// Create an empty bundle
    pub fn new() -> Self {
        Self {
            transactions: [Transaction::new_unchecked(0, 1, 1, 0, 0); BUNDLE_MAX],
            count: 0,
            _padding: 0,
            timestamp_ns: 0,
        }
    }

    /// Create a bundle with transactions and validation
    ///
    /// # Errors
    /// - `CountTooLarge`: if count exceeds BUNDLE_MAX
    pub fn with_transactions(
        transactions: [Transaction; BUNDLE_MAX],
        count: u32,
        timestamp_ns: u64
    ) -> Result<Self, BundleError> {
        // Validate count
        if count as usize > BUNDLE_MAX {
            return Err(BundleError::CountTooLarge {
                count,
                max: BUNDLE_MAX,
            });
        }

        Ok(Self {
            transactions,
            count,
            _padding: 0,
            timestamp_ns,
        })
    }

    /// Create a bundle without validation (for trusted inputs)
    pub fn with_transactions_unchecked(
        transactions: [Transaction; BUNDLE_MAX],
        count: u32,
        timestamp_ns: u64
    ) -> Self {
        debug_assert!(count as usize <= BUNDLE_MAX, "count must not exceed BUNDLE_MAX");

        Self {
            transactions,
            count,
            _padding: 0,
            timestamp_ns,
        }
    }

    /// Get active transactions (only up to count)
    pub fn active_transactions(&self) -> &[Transaction] {
        &self.transactions[..self.count as usize]
    }

    /// Check if bundle is empty
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Check if bundle is full
    pub fn is_full(&self) -> bool {
        self.count as usize >= BUNDLE_MAX
    }
}

impl Default for Bundle {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Bundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bundle")
            .field("count", &self.count)
            .field("timestamp_ns", &self.timestamp_ns)
            .field("transactions", &self.active_transactions())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_layout() {
        assert_eq!(core::mem::size_of::<Transaction>(), 32);
        assert_eq!(core::mem::align_of::<Transaction>(), 8);
    }

    #[test]
    fn test_transaction_serialization() {
        let txn = Transaction::new_unchecked(123, 1000000, 50, 0, 1234567890);
        let bytes = txn.to_bytes();
        let txn2 = Transaction::from_bytes(&bytes);

        assert_eq!(txn.id, txn2.id);
        assert_eq!(txn.price, txn2.price);
        assert_eq!(txn.size, txn2.size);
        assert_eq!(txn.side, txn2.side);
        assert_eq!(txn.ingress_ts_ns, txn2.ingress_ts_ns);
    }

    #[test]
    fn test_transaction_price_conversion() {
        let txn = Transaction::new_unchecked(1, 950000, 100, 0, 0);
        assert_eq!(txn.price_f64(), 95.0);
    }

    #[test]
    fn test_transaction_validation() {
        // Valid transaction
        assert!(Transaction::new(1, 1000, 100, 0, 0).is_ok());
        assert!(Transaction::new(1, 1000, 100, 1, 0).is_ok());

        // Invalid side
        assert_eq!(
            Transaction::new(1, 1000, 100, 2, 0),
            Err(TransactionError::InvalidSide(2))
        );

        // Negative price
        assert_eq!(
            Transaction::new(1, -1000, 100, 0, 0),
            Err(TransactionError::NegativePrice(-1000))
        );

        // Zero price
        assert_eq!(
            Transaction::new(1, 0, 100, 0, 0),
            Err(TransactionError::NegativePrice(0))
        );

        // Zero size
        assert_eq!(
            Transaction::new(1, 1000, 0, 0, 0),
            Err(TransactionError::ZeroSize)
        );
    }

    #[test]
    fn test_bundle_validation() {
        let txns = [Transaction::new_unchecked(0, 1, 1, 0, 0); BUNDLE_MAX];

        // Valid bundle
        assert!(Bundle::with_transactions(txns, 16, 0).is_ok());
        assert!(Bundle::with_transactions(txns, 0, 0).is_ok());

        // Count too large
        assert_eq!(
            Bundle::with_transactions(txns, 17, 0),
            Err(BundleError::CountTooLarge { count: 17, max: 16 })
        );
    }

    #[test]
    fn test_bundle_layout() {
        let size = core::mem::size_of::<Bundle>();
        assert_eq!(size, 32 * BUNDLE_MAX + 16);
    }

    #[test]
    fn test_bundle_creation() {
        let bundle = Bundle::new();
        assert_eq!(bundle.count, 0);
        assert!(bundle.is_empty());
        assert!(!bundle.is_full());
    }
}

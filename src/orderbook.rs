use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use static_assertions::const_assert;
use crate::errors::OrderBookError;

/// Number of price levels in the order book
const LEVELS: usize = 1024;

/// Tick shift for price bucketing (each level = 2^4 = 16 ticks)
const TICK_SHIFT: u32 = 4;

/// Mask for level indexing
const LEVEL_MASK: usize = LEVELS - 1;

/// Maximum CAS retry attempts before timeout
const MAX_RETRIES: usize = 100;

// Verify LEVELS is power of 2
const_assert!((LEVELS & (LEVELS - 1)) == 0);

/// Cache-line padded atomic for best bid/ask tracking
#[repr(C, align(64))]
struct CachePadded<T> {
    value: T,
}

impl<T> CachePadded<T> {
    fn new(value: T) -> Self {
        Self { value }
    }
}

/// A single price level in the order book
#[repr(C, align(64))]
struct PriceLevel {
    /// Net quantity at this level (can be negative)
    quantity: AtomicI64,
    /// Last update timestamp (TSC or nanoseconds)
    timestamp: AtomicU64,
}

impl PriceLevel {
    #[allow(dead_code)]
    fn new() -> Self {
        Self {
            quantity: AtomicI64::new(0),
            timestamp: AtomicU64::new(0),
        }
    }
}

// Timeout error moved to errors.rs

/// Lock-free order book with fixed-size price levels.
/// Uses CAS loops for atomic updates.
///
/// # IMPORTANT: This is a Price-Aggregated Order Book
///
/// This implementation uses **price bucketing** where multiple prices
/// map to the same storage level. This makes it fast but approximate.
///
/// **Key limitations**:
/// - Multiple prices (16 ticks) share the same bucket
/// - Cannot reconstruct individual price levels
/// - Best bid/ask are approximate (within ±15 ticks)
/// - No price-time priority
///
/// **Good for**: High-frequency analytics, volume tracking, MEV detection
/// **NOT for**: Order matching, precise P&L, limit order books
///
/// See `ORDERBOOK_LIMITATIONS.md` for detailed explanation.
pub struct OrderBook {
    /// Bid side (buy orders)
    bids: [PriceLevel; LEVELS],
    /// Ask side (sell orders)
    asks: [PriceLevel; LEVELS],
    /// Best bid price (highest)
    best_bid: CachePadded<AtomicI64>,
    /// Best ask price (lowest)
    best_ask: CachePadded<AtomicI64>,
}

impl OrderBook {
    /// Create a new order book
    pub fn new() -> Self {
        // Initialize arrays with default values
        const INIT: PriceLevel = PriceLevel {
            quantity: AtomicI64::new(0),
            timestamp: AtomicU64::new(0),
        };

        Self {
            bids: [INIT; LEVELS],
            asks: [INIT; LEVELS],
            best_bid: CachePadded::new(AtomicI64::new(0)),
            best_ask: CachePadded::new(AtomicI64::new(i64::MAX)),
        }
    }

    /// Map price to level index using bit shift and mask
    #[inline(always)]
    fn level_index(price: i64) -> usize {
        ((price >> TICK_SHIFT) as usize) & LEVEL_MASK
    }

    /// Update a bid level with delta quantity.
    /// Uses bounded CAS retry with exponential backoff.
    pub fn update_bid(&self, price: i64, delta: i64, timestamp: u64) -> Result<(), OrderBookError> {
        let idx = Self::level_index(price);
        let level = &self.bids[idx];

        let mut backoff = 1;
        for _ in 0..MAX_RETRIES {
            let current = level.quantity.load(Ordering::Acquire);

            // Check for overflow before adding
            let new_qty = current.checked_add(delta)
                .ok_or(OrderBookError::QuantityOverflow)?;

            // Try to update quantity with CAS
            match level.quantity.compare_exchange_weak(
                current,
                new_qty,
                Ordering::Release,  // Success: synchronize with other threads
                Ordering::Relaxed,  // Failure: retry anyway
            ) {
                Ok(_) => {
                    // Update timestamp (relaxed is fine, not critical)
                    level.timestamp.store(timestamp, Ordering::Relaxed);

                    // Update best bid if necessary
                    self.update_best_bid(price, new_qty);
                    return Ok(());
                }
                Err(_) => {
                    // Exponential backoff with spin_loop hint
                    for _ in 0..backoff {
                        core::hint::spin_loop();
                    }
                    backoff = (backoff * 2).min(64);
                }
            }
        }

        Err(OrderBookError::Timeout)
    }

    /// Update an ask level with delta quantity
    pub fn update_ask(&self, price: i64, delta: i64, timestamp: u64) -> Result<(), OrderBookError> {
        let idx = Self::level_index(price);
        let level = &self.asks[idx];

        let mut backoff = 1;
        for _ in 0..MAX_RETRIES {
            let current = level.quantity.load(Ordering::Acquire);

            // Check for overflow before adding
            let new_qty = current.checked_add(delta)
                .ok_or(OrderBookError::QuantityOverflow)?;

            match level.quantity.compare_exchange_weak(
                current,
                new_qty,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    level.timestamp.store(timestamp, Ordering::Relaxed);
                    self.update_best_ask(price, new_qty);
                    return Ok(());
                }
                Err(_) => {
                    for _ in 0..backoff {
                        core::hint::spin_loop();
                    }
                    backoff = (backoff * 2).min(64);
                }
            }
        }

        Err(OrderBookError::Timeout)
    }

    /// Update best bid price if needed (optimistic, may be slightly stale)
    fn update_best_bid(&self, price: i64, new_qty: i64) {
        if new_qty > 0 {
            // Level has quantity, potentially update best bid
            let mut current_best = self.best_bid.value.load(Ordering::Relaxed);
            while price > current_best {
                match self.best_bid.value.compare_exchange_weak(
                    current_best,
                    price,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(x) => current_best = x,
                }
            }
        } else {
            // Level is now empty, may need to scan for new best bid
            let current_best = self.best_bid.value.load(Ordering::Relaxed);
            if price == current_best {
                // Lost the best bid, expensive scan needed (simplified: just clear)
                self.best_bid.value.store(0, Ordering::Relaxed);
            }
        }
    }

    /// Update best ask price if needed
    fn update_best_ask(&self, price: i64, new_qty: i64) {
        if new_qty > 0 {
            let mut current_best = self.best_ask.value.load(Ordering::Relaxed);
            while price < current_best {
                match self.best_ask.value.compare_exchange_weak(
                    current_best,
                    price,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(x) => current_best = x,
                }
            }
        } else {
            let current_best = self.best_ask.value.load(Ordering::Relaxed);
            if price == current_best {
                self.best_ask.value.store(i64::MAX, Ordering::Relaxed);
            }
        }
    }

    /// Get current best bid price (may be slightly stale)
    pub fn best_bid(&self) -> i64 {
        self.best_bid.value.load(Ordering::Relaxed)
    }

    /// Get current best ask price (may be slightly stale)
    pub fn best_ask(&self) -> i64 {
        self.best_ask.value.load(Ordering::Relaxed)
    }

    /// Get quantity at a specific bid level
    pub fn bid_quantity(&self, price: i64) -> i64 {
        let idx = Self::level_index(price);
        self.bids[idx].quantity.load(Ordering::Acquire)
    }

    /// Get quantity at a specific ask level
    pub fn ask_quantity(&self, price: i64) -> i64 {
        let idx = Self::level_index(price);
        self.asks[idx].quantity.load(Ordering::Acquire)
    }

    /// Get spread (best_ask - best_bid)
    pub fn spread(&self) -> i64 {
        let bid = self.best_bid();
        let ask = self.best_ask();
        if ask == i64::MAX || bid == 0 {
            return 0;
        }
        ask - bid
    }
}

// Safety: OrderBook can be shared between threads
unsafe impl Send for OrderBook {}
unsafe impl Sync for OrderBook {}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_index() {
        // Tick shift = 4, so prices 0-15 map to index 0
        assert_eq!(OrderBook::level_index(0), 0);
        assert_eq!(OrderBook::level_index(15), 0);
        assert_eq!(OrderBook::level_index(16), 1);
        assert_eq!(OrderBook::level_index(32), 2);
    }

    #[test]
    fn test_bid_update() {
        let book = OrderBook::new();

        assert!(book.update_bid(1000, 100, 123).is_ok());
        assert_eq!(book.bid_quantity(1000), 100);

        assert!(book.update_bid(1000, 50, 124).is_ok());
        assert_eq!(book.bid_quantity(1000), 150);
    }

    #[test]
    fn test_ask_update() {
        let book = OrderBook::new();

        assert!(book.update_ask(2000, 100, 123).is_ok());
        assert_eq!(book.ask_quantity(2000), 100);

        assert!(book.update_ask(2000, -50, 124).is_ok());
        assert_eq!(book.ask_quantity(2000), 50);
    }

    #[test]
    fn test_best_bid_ask() {
        let book = OrderBook::new();

        book.update_bid(1000, 100, 1).unwrap();
        assert_eq!(book.best_bid(), 1000);

        book.update_bid(1100, 50, 2).unwrap();
        assert_eq!(book.best_bid(), 1100);

        book.update_ask(2000, 100, 3).unwrap();
        assert_eq!(book.best_ask(), 2000);

        book.update_ask(1900, 75, 4).unwrap();
        assert_eq!(book.best_ask(), 1900);
    }

    #[test]
    fn test_spread() {
        let book = OrderBook::new();

        book.update_bid(1000, 100, 1).unwrap();
        book.update_ask(1100, 100, 2).unwrap();

        assert_eq!(book.spread(), 100);
    }
}

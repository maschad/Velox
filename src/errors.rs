/// Error types for the velox-engine
use core::fmt;

/// Errors that can occur when creating a Transaction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionError {
    /// Side must be 0 (bid) or 1 (ask)
    InvalidSide(u8),
    /// Price must be positive
    NegativePrice(i64),
    /// Size must be non-zero
    ZeroSize,
}

impl fmt::Display for TransactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSide(side) => write!(f, "Invalid side: {} (must be 0 or 1)", side),
            Self::NegativePrice(price) => write!(f, "Negative price: {} (must be positive)", price),
            Self::ZeroSize => write!(f, "Zero size (must be non-zero)"),
        }
    }
}

impl std::error::Error for TransactionError {}

/// Errors that can occur when creating a Bundle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleError {
    /// Count exceeds BUNDLE_MAX
    CountTooLarge { count: u32, max: usize },
}

impl fmt::Display for BundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountTooLarge { count, max } => {
                write!(f, "Bundle count {} exceeds maximum {}", count, max)
            }
        }
    }
}

impl std::error::Error for BundleError {}

/// Errors that can occur in the order book
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderBookError {
    /// Quantity overflow when adding orders
    QuantityOverflow,
    /// CAS loop exceeded maximum retries
    Timeout,
}

impl fmt::Display for OrderBookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QuantityOverflow => write!(f, "Order book quantity overflow"),
            Self::Timeout => write!(f, "CAS operation timed out after max retries"),
        }
    }
}

impl std::error::Error for OrderBookError {}

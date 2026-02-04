/// Adaptive backoff strategy for spin loops
///
/// Starts with spinning (low latency), then yields to OS (low CPU usage).
/// This balances latency vs CPU consumption.
use core::hint::spin_loop;
use std::thread;

/// Adaptive backoff iterator
///
/// Usage:
/// ```
/// let mut backoff = Backoff::new();
/// loop {
///     if try_operation() {
///         break;
///     }
///     backoff.snooze();
/// }
/// ```
pub struct Backoff {
    step: u32,
}

const SPIN_LIMIT: u32 = 6;  // Spin for 2^6 = 64 iterations
const YIELD_LIMIT: u32 = 10; // Yield for 4 iterations before parking

impl Backoff {
    /// Create a new backoff strategy
    pub fn new() -> Self {
        Self { step: 0 }
    }

    /// Reset the backoff state
    pub fn reset(&mut self) {
        self.step = 0;
    }

    /// Check if we should spin (true) or yield/park (false)
    pub fn is_spinning(&self) -> bool {
        self.step <= SPIN_LIMIT
    }

    /// Perform one step of backoff
    pub fn snooze(&mut self) {
        if self.step <= SPIN_LIMIT {
            // Phase 1: Spin with exponential backoff
            // 1, 2, 4, 8, 16, 32, 64 spin loops
            for _ in 0..(1 << self.step) {
                spin_loop();
            }
        } else if self.step <= YIELD_LIMIT {
            // Phase 2: Yield to OS scheduler
            // This gives up our time slice but keeps thread runnable
            thread::yield_now();
        } else {
            // Phase 3: Park for 1ms (long idle)
            // For truly idle cases, reduce CPU usage to near zero
            thread::sleep(std::time::Duration::from_micros(100));
        }

        // Cap the step to prevent overflow
        self.step = self.step.saturating_add(1).min(YIELD_LIMIT + 1);
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_phases() {
        let mut backoff = Backoff::new();

        // Should start spinning
        assert!(backoff.is_spinning());

        // Step through spin phase
        for _ in 0..SPIN_LIMIT {
            backoff.snooze();
        }

        // Should transition to yield phase
        backoff.snooze();
        assert!(!backoff.is_spinning());
    }

    #[test]
    fn test_backoff_reset() {
        let mut backoff = Backoff::new();

        // Advance to yield phase
        for _ in 0..10 {
            backoff.snooze();
        }
        assert!(!backoff.is_spinning());

        // Reset should go back to spinning
        backoff.reset();
        assert!(backoff.is_spinning());
    }
}

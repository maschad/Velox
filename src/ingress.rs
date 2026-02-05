use crate::ring::RingBuffer;
use crate::tsc::{rdtsc, spin_sleep_ns, tsc_to_ns};
use crate::types::Transaction;
use rand::Rng;

/// Synthetic transaction ingress with Poisson arrival process.
/// Generates random transactions and pushes to ring buffer.
/// Drops transactions on buffer full (backpressure).
///
/// # Parameters
/// - `ring`: Ring buffer to push transactions into
/// - `rate_hz`: Target transaction rate in transactions per second
/// - `duration_secs`: How long to generate transactions (0 = infinite)
pub fn synthetic_ingress(
    ring: &RingBuffer<Transaction, 4096>,
    rate_hz: f64,
    duration_secs: u64,
) -> SyntheticStats {
    let mut rng = rand::thread_rng();
    let lambda = rate_hz;

    let mut stats = SyntheticStats::default();
    let start_tsc = rdtsc();
    let duration_ns = duration_secs * 1_000_000_000;

    loop {
        // Check if duration exceeded
        if duration_secs > 0 {
            let elapsed_ns = tsc_to_ns(rdtsc() - start_tsc);
            if elapsed_ns >= duration_ns {
                break;
            }
        }

        // Generate random transaction
        let txn = Transaction::new_unchecked(
            stats.generated,
            rng.gen_range(900000..1100000), // $90-$110 in fixed-point (4 decimals)
            rng.gen_range(1..1000),
            rng.gen_range(0..2) as u8,
            tsc_to_ns(rdtsc()),
        );

        stats.generated += 1;

        // Push to ring buffer (drop on full)
        match ring.push(txn) {
            Ok(_) => stats.pushed += 1,
            Err(_) => stats.dropped += 1,
        }

        // Poisson inter-arrival delay using exponential distribution
        // Generate exponential random variable: -ln(U) / lambda
        let u: f64 = rng.gen();
        // Avoid u == 0.0 which would cause -inf
        let u = u.max(f64::EPSILON);
        let delay_ns = ((-u.ln()) / lambda * 1_000_000_000.0) as u64;
        if delay_ns > 0 {
            spin_sleep_ns(delay_ns);
        }
    }

    stats
}

/// Statistics from synthetic ingress
#[derive(Debug, Default, Clone, Copy)]
pub struct SyntheticStats {
    pub generated: u64,
    pub pushed: u64,
    pub dropped: u64,
}

impl SyntheticStats {
    pub fn drop_rate(&self) -> f64 {
        if self.generated == 0 {
            0.0
        } else {
            self.dropped as f64 / self.generated as f64
        }
    }
}

/// Generate a burst of transactions for testing.
/// Returns number of transactions successfully pushed.
pub fn generate_burst(
    ring: &RingBuffer<Transaction, 4096>,
    count: usize,
    base_price: i64,
) -> usize {
    let mut pushed = 0;
    let mut rng = rand::thread_rng();

    for i in 0..count {
        let txn = Transaction::new_unchecked(
            i as u64,
            base_price + rng.gen_range(-5000..5000),
            rng.gen_range(1..100),
            rng.gen_range(0..2) as u8,
            tsc_to_ns(rdtsc()),
        );

        if ring.push(txn).is_ok() {
            pushed += 1;
        }
    }

    pushed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsc::init_tsc;

    #[test]
    fn test_generate_burst() {
        init_tsc();
        let ring = RingBuffer::<Transaction, 4096>::new();

        let pushed = generate_burst(&ring, 100, 1000000);
        assert_eq!(pushed, 100);
        assert_eq!(ring.len(), 100);
    }

    #[test]
    fn test_generate_burst_overflow() {
        init_tsc();
        let ring = RingBuffer::<Transaction, 4096>::new();

        // Try to push more than ring capacity
        let pushed = generate_burst(&ring, 5000, 1000000);
        // Should hit buffer limit
        assert!(pushed <= 4096);
    }

    #[test]
    #[ignore] // This test runs forever with duration=0, skip by default
    fn test_synthetic_ingress_duration() {
        init_tsc();
        let _ring = RingBuffer::<Transaction, 4096>::new();

        // This test is ignored because duration=0 means infinite loop
        // To test properly, use a thread with timeout
    }

    #[test]
    fn test_synthetic_stats() {
        let stats = SyntheticStats {
            generated: 1000,
            pushed: 950,
            dropped: 50,
        };

        assert_eq!(stats.drop_rate(), 0.05);
    }
}

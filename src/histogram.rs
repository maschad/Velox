use core::sync::atomic::{AtomicU64, Ordering};

/// Cache-line padded wrapper to prevent false sharing between buckets
#[repr(C, align(64))]
struct CachePadded<T> {
    value: T,
}

impl<T> CachePadded<T> {
    fn new(value: T) -> Self {
        Self { value }
    }
}

/// Lock-free latency histogram with logarithmic buckets.
///
/// Tracks latency distribution using 13 buckets spanning 0ns to 500+μs.
/// All operations are wait-free (single atomic increment per record).
///
/// Bucket ranges:
/// 0: [0, 100) ns
/// 1: [100, 200) ns
/// 2: [200, 500) ns
/// 3: [500, 1000) ns (1μs)
/// 4: [1, 2) μs
/// 5: [2, 5) μs
/// 6: [5, 10) μs
/// 7: [10, 20) μs
/// 8: [20, 50) μs
/// 9: [50, 100) μs
/// 10: [100, 200) μs
/// 11: [200, 500) μs
/// 12: [500+) μs
pub struct LatencyHistogram {
    /// Cache-line padded buckets to prevent false sharing
    buckets: [CachePadded<AtomicU64>; 13],
    /// Total number of samples recorded
    total_samples: CachePadded<AtomicU64>,
    /// Sum of all latencies in nanoseconds
    total_latency_ns: CachePadded<AtomicU64>,
    /// Minimum latency observed
    min_latency_ns: CachePadded<AtomicU64>,
    /// Maximum latency observed
    max_latency_ns: CachePadded<AtomicU64>,
}

impl LatencyHistogram {
    /// Create a new histogram with all buckets initialized to zero
    pub fn new() -> Self {
        Self {
            buckets: [
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
                CachePadded::new(AtomicU64::new(0)),
            ],
            total_samples: CachePadded::new(AtomicU64::new(0)),
            total_latency_ns: CachePadded::new(AtomicU64::new(0)),
            min_latency_ns: CachePadded::new(AtomicU64::new(u64::MAX)),
            max_latency_ns: CachePadded::new(AtomicU64::new(0)),
        }
    }

    /// Select bucket index for given latency in nanoseconds.
    /// Uses binary search approach for O(1) bucket selection.
    fn bucket_index(latency_ns: u64) -> usize {
        match latency_ns {
            0..=99 => 0,
            100..=199 => 1,
            200..=499 => 2,
            500..=999 => 3,
            1_000..=1_999 => 4,
            2_000..=4_999 => 5,
            5_000..=9_999 => 6,
            10_000..=19_999 => 7,
            20_000..=49_999 => 8,
            50_000..=99_999 => 9,
            100_000..=199_999 => 10,
            200_000..=499_999 => 11,
            _ => 12, // 500μs+
        }
    }

    /// Record a latency sample. Wait-free operation.
    ///
    /// # Arguments
    /// * `latency_ns` - Latency in nanoseconds
    pub fn record(&self, latency_ns: u64) {
        let bucket = Self::bucket_index(latency_ns);
        self.buckets[bucket].value.fetch_add(1, Ordering::Relaxed);
        self.total_samples.value.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ns.value.fetch_add(latency_ns, Ordering::Relaxed);

        // Update min (optimistic, may lose some races but that's acceptable)
        let mut current_min = self.min_latency_ns.value.load(Ordering::Relaxed);
        while latency_ns < current_min {
            match self.min_latency_ns.value.compare_exchange_weak(
                current_min,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_min = actual,
            }
        }

        // Update max (optimistic, may lose some races but that's acceptable)
        let mut current_max = self.max_latency_ns.value.load(Ordering::Relaxed);
        while latency_ns > current_max {
            match self.max_latency_ns.value.compare_exchange_weak(
                current_max,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_max = actual,
            }
        }
    }

    /// Calculate percentile from histogram.
    ///
    /// # Arguments
    /// * `p` - Percentile as fraction (0.0 to 1.0)
    ///
    /// # Returns
    /// Estimated latency in nanoseconds at the given percentile
    pub fn percentile(&self, p: f64) -> u64 {
        let total = self.total_samples.value.load(Ordering::Relaxed);
        if total == 0 {
            return 0;
        }

        let target_count = (total as f64 * p) as u64;
        let mut cumulative = 0u64;

        for (i, bucket) in self.buckets.iter().enumerate() {
            cumulative += bucket.value.load(Ordering::Relaxed);
            if cumulative >= target_count {
                // Return midpoint of bucket range
                return match i {
                    0 => 50,           // [0, 100)
                    1 => 150,          // [100, 200)
                    2 => 350,          // [200, 500)
                    3 => 750,          // [500, 1000)
                    4 => 1_500,        // [1, 2) μs
                    5 => 3_500,        // [2, 5) μs
                    6 => 7_500,        // [5, 10) μs
                    7 => 15_000,       // [10, 20) μs
                    8 => 35_000,       // [20, 50) μs
                    9 => 75_000,       // [50, 100) μs
                    10 => 150_000,     // [100, 200) μs
                    11 => 350_000,     // [200, 500) μs
                    _ => 750_000,      // [500+) μs
                };
            }
        }

        // All samples in last bucket
        750_000
    }

    /// Print comprehensive summary statistics
    pub fn print_summary(&self) {
        let total = self.total_samples.value.load(Ordering::Relaxed);
        if total == 0 {
            println!("No latency samples recorded");
            return;
        }

        let total_latency = self.total_latency_ns.value.load(Ordering::Relaxed);
        let mean_ns = total_latency / total;
        let min_ns = self.min_latency_ns.value.load(Ordering::Relaxed);
        let max_ns = self.max_latency_ns.value.load(Ordering::Relaxed);

        let p50 = self.percentile(0.50);
        let p95 = self.percentile(0.95);
        let p99 = self.percentile(0.99);
        let p999 = self.percentile(0.999);

        println!("\n=== Latency Distribution ===");
        println!("Samples: {}", total);
        println!("Mean:    {} ns ({:.2} μs)", mean_ns, mean_ns as f64 / 1_000.0);
        println!("Min:     {} ns ({:.2} μs)", min_ns, min_ns as f64 / 1_000.0);
        println!("Max:     {} ns ({:.2} μs)", max_ns, max_ns as f64 / 1_000.0);
        println!("\nPercentiles:");
        println!("  P50:   {} ns ({:.2} μs)", p50, p50 as f64 / 1_000.0);
        println!("  P95:   {} ns ({:.2} μs)", p95, p95 as f64 / 1_000.0);
        println!("  P99:   {} ns ({:.2} μs)", p99, p99 as f64 / 1_000.0);
        println!("  P99.9: {} ns ({:.2} μs)", p999, p999 as f64 / 1_000.0);

        println!("\nDistribution:");
        let bucket_names = [
            "0-100ns", "100-200ns", "200-500ns", "500-1000ns",
            "1-2μs", "2-5μs", "5-10μs", "10-20μs", "20-50μs",
            "50-100μs", "100-200μs", "200-500μs", "500+μs",
        ];

        for (i, bucket) in self.buckets.iter().enumerate() {
            let count = bucket.value.load(Ordering::Relaxed);
            if count > 0 {
                let pct = (count as f64 / total as f64) * 100.0;
                let bar_len = (pct * 0.5) as usize; // Scale for terminal width
                let bar: String = "█".repeat(bar_len);
                println!("  {:<12} {:>8} ({:>5.2}%) {}",
                    bucket_names[i], count, pct, bar);
            }
        }
        println!();
    }

    /// Reset all counters to zero
    pub fn reset(&self) {
        for bucket in &self.buckets {
            bucket.value.store(0, Ordering::Relaxed);
        }
        self.total_samples.value.store(0, Ordering::Relaxed);
        self.total_latency_ns.value.store(0, Ordering::Relaxed);
        self.min_latency_ns.value.store(u64::MAX, Ordering::Relaxed);
        self.max_latency_ns.value.store(0, Ordering::Relaxed);
    }
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_selection() {
        assert_eq!(LatencyHistogram::bucket_index(0), 0);
        assert_eq!(LatencyHistogram::bucket_index(50), 0);
        assert_eq!(LatencyHistogram::bucket_index(99), 0);
        assert_eq!(LatencyHistogram::bucket_index(100), 1);
        assert_eq!(LatencyHistogram::bucket_index(199), 1);
        assert_eq!(LatencyHistogram::bucket_index(200), 2);
        assert_eq!(LatencyHistogram::bucket_index(499), 2);
        assert_eq!(LatencyHistogram::bucket_index(500), 3);
        assert_eq!(LatencyHistogram::bucket_index(999), 3);
        assert_eq!(LatencyHistogram::bucket_index(1_000), 4);
        assert_eq!(LatencyHistogram::bucket_index(1_999), 4);
        assert_eq!(LatencyHistogram::bucket_index(2_000), 5);
        assert_eq!(LatencyHistogram::bucket_index(10_000), 7);
        assert_eq!(LatencyHistogram::bucket_index(50_000), 9);
        assert_eq!(LatencyHistogram::bucket_index(500_000), 12);
        assert_eq!(LatencyHistogram::bucket_index(1_000_000), 12);
    }

    #[test]
    fn test_record_and_percentiles() {
        let hist = LatencyHistogram::new();

        // Record samples in known distribution
        for _ in 0..100 {
            hist.record(50); // Bucket 0
        }
        for _ in 0..50 {
            hist.record(150); // Bucket 1
        }
        for _ in 0..30 {
            hist.record(300); // Bucket 2
        }
        for _ in 0..20 {
            hist.record(700); // Bucket 3
        }

        // Total: 200 samples
        // Cumulative: [100, 150, 180, 200]
        // P50 should be in bucket 0 (100/200 = 50%)
        // P75 should be in bucket 1 (150/200 = 75%)
        // P90 should be in bucket 2 (180/200 = 90%)
        // P99 should be in bucket 3 (198/200 = 99%)

        assert_eq!(hist.percentile(0.50), 50); // Bucket 0 midpoint
        assert_eq!(hist.percentile(0.75), 150); // Bucket 1 midpoint
        assert_eq!(hist.percentile(0.90), 350); // Bucket 2 midpoint
        assert_eq!(hist.percentile(0.99), 750); // Bucket 3 midpoint
    }

    #[test]
    fn test_min_max_tracking() {
        let hist = LatencyHistogram::new();

        hist.record(1_000);
        hist.record(500);
        hist.record(2_000);
        hist.record(100);
        hist.record(5_000);

        assert_eq!(hist.min_latency_ns.value.load(Ordering::Relaxed), 100);
        assert_eq!(hist.max_latency_ns.value.load(Ordering::Relaxed), 5_000);
    }

    #[test]
    fn test_mean_calculation() {
        let hist = LatencyHistogram::new();

        hist.record(100);
        hist.record(200);
        hist.record(300);
        hist.record(400);

        let total = hist.total_samples.value.load(Ordering::Relaxed);
        let total_latency = hist.total_latency_ns.value.load(Ordering::Relaxed);
        let mean = total_latency / total;

        assert_eq!(total, 4);
        assert_eq!(total_latency, 1_000);
        assert_eq!(mean, 250);
    }

    #[test]
    fn test_reset() {
        let hist = LatencyHistogram::new();

        hist.record(100);
        hist.record(200);
        hist.record(300);

        hist.reset();

        assert_eq!(hist.total_samples.value.load(Ordering::Relaxed), 0);
        assert_eq!(hist.total_latency_ns.value.load(Ordering::Relaxed), 0);
        assert_eq!(hist.min_latency_ns.value.load(Ordering::Relaxed), u64::MAX);
        assert_eq!(hist.max_latency_ns.value.load(Ordering::Relaxed), 0);

        for bucket in &hist.buckets {
            assert_eq!(bucket.value.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn test_empty_histogram() {
        let hist = LatencyHistogram::new();

        assert_eq!(hist.percentile(0.50), 0);
        assert_eq!(hist.percentile(0.99), 0);
    }

    #[test]
    fn test_single_bucket_distribution() {
        let hist = LatencyHistogram::new();

        // All samples in last bucket
        for _ in 0..100 {
            hist.record(1_000_000);
        }

        assert_eq!(hist.percentile(0.50), 750_000);
        assert_eq!(hist.percentile(0.99), 750_000);
    }
}

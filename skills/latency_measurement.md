# Latency Measurement

Measure sub-microsecond latencies using architecture-specific timestamp counters (TSC) and wait-free histograms. Essential for real-time systems, HFT, and latency-sensitive applications.

## When to use

- Sub-microsecond latency requirements (HFT, real-time systems)
- Hot path measurement where allocation is unacceptable
- P99/P95 percentile analysis (not just averages)
- Concurrent measurement (multiple threads recording simultaneously)
- When `std::time::Instant` is too slow or imprecise
- Latency-sensitive request-response systems
- Performance regression detection in CI/CD

## When NOT to use

- Millisecond-scale latencies (Instant is sufficient)
- Cold paths where allocation is acceptable
- Non-performance-critical code
- When standard deviation is more important than percentiles
- Cross-platform code where TSC isn't available (use fallback)
- Long-duration measurements (TSC can drift, use wall-clock time)

## Instructions

### Step 1: Implement Architecture-Specific TSC Reading

The Time Stamp Counter (TSC) provides nanosecond-resolution timing on modern CPUs:

**ARM64 (Apple Silicon, AWS Graviton)**:
```rust
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub fn rdtsc() -> u64 {
    let tsc: u64;
    unsafe {
        core::arch::asm!(
            "mrs {}, cntvct_el0",
            out(reg) tsc,
            options(nomem, nostack)
        );
    }
    tsc
}
```

**x86_64 (Intel, AMD)**:
```rust
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}
```

**Fallback (other architectures)**:
```rust
#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
#[inline(always)]
pub fn rdtsc() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}
```

**Key points**:
- `inline(always)` ensures minimal overhead (1-2 cycles)
- ARM64 uses `cntvct_el0` (virtual counter, ~1ns resolution)
- x86_64 uses `_rdtsc` intrinsic (direct RDTSC instruction)
- Fallback provides compatibility but lower precision

### Step 2: Calibrate TSC to Nanoseconds

TSC returns CPU ticks, not nanoseconds. Calibrate once at startup:

```rust
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use std::thread;

/// TSC calibration factor (TSC ticks per nanosecond)
static TSC_PER_NS: OnceLock<f64> = OnceLock::new();

/// Calibrate TSC by measuring ticks over a known duration
pub fn calibrate_tsc() -> f64 {
    let start_tsc = rdtsc();
    let start = Instant::now();

    // Sleep for 100ms to get accurate calibration
    thread::sleep(Duration::from_millis(100));

    let end_tsc = rdtsc();
    let elapsed_ns = start.elapsed().as_nanos() as u64;

    let tsc_per_ns = (end_tsc - start_tsc) as f64 / elapsed_ns as f64;
    tsc_per_ns
}

/// Initialize TSC calibration (call once at startup)
pub fn init_tsc() {
    TSC_PER_NS.get_or_init(|| calibrate_tsc());
}

/// Check if TSC has been initialized
pub fn is_tsc_initialized() -> bool {
    TSC_PER_NS.get().is_some()
}
```

**Calibration details**:
- 100ms sleep provides accurate calibration (within 1%)
- `OnceLock` ensures calibration happens exactly once
- Thread-safe initialization (first caller calibrates)

### Step 3: Create Inline Conversion Functions

Convert between TSC ticks and nanoseconds:

```rust
/// Convert TSC ticks to nanoseconds
#[inline(always)]
pub fn tsc_to_ns(tsc: u64) -> u64 {
    let factor = TSC_PER_NS.get().expect(
        "TSC not calibrated - call init_tsc() before using"
    );
    (tsc as f64 / factor) as u64
}

/// Convert nanoseconds to TSC ticks
#[inline(always)]
pub fn ns_to_tsc(ns: u64) -> u64 {
    let factor = TSC_PER_NS.get().expect("TSC not calibrated");
    (ns as f64 * factor) as u64
}
```

**Usage example**:
```rust
fn main() {
    init_tsc();  // MUST call this first!

    let start = rdtsc();
    expensive_operation();
    let end = rdtsc();

    let latency_ns = tsc_to_ns(end - start);
    println!("Latency: {}ns ({:.2}μs)", latency_ns, latency_ns as f64 / 1000.0);
}
```

### Step 4: Design Logarithmic Histogram Buckets

For wide latency ranges (0-500μs), use logarithmic buckets:

```rust
pub struct LatencyHistogram {
    buckets: [CachePadded<AtomicU64>; 13],
    // ... other fields
}

impl LatencyHistogram {
    /// Select bucket index for given latency in nanoseconds
    fn bucket_index(latency_ns: u64) -> usize {
        match latency_ns {
            0..=99 => 0,           // [0, 100) ns
            100..=199 => 1,        // [100, 200) ns
            200..=499 => 2,        // [200, 500) ns
            500..=999 => 3,        // [500, 1000) ns = [0.5, 1) μs
            1_000..=1_999 => 4,    // [1, 2) μs
            2_000..=4_999 => 5,    // [2, 5) μs
            5_000..=9_999 => 6,    // [5, 10) μs
            10_000..=19_999 => 7,  // [10, 20) μs
            20_000..=49_999 => 8,  // [20, 50) μs
            50_000..=99_999 => 9,  // [50, 100) μs
            100_000..=199_999 => 10, // [100, 200) μs
            200_000..=499_999 => 11, // [200, 500) μs
            _ => 12,               // [500+) μs
        }
    }
}
```

**Why logarithmic**:
- Covers wide range (0-500+μs) with only 13 buckets
- Higher precision for low latencies (where it matters)
- Lower precision for high latencies (outliers)
- Memory-efficient (13 * 64 bytes = 832 bytes)

### Step 5: Implement Wait-Free Histogram Recording

Recording must never block, even with multiple threads:

```rust
use core::sync::atomic::{AtomicU64, Ordering};

#[repr(C, align(64))]
struct CachePadded<T> {
    value: T,
}

pub struct LatencyHistogram {
    buckets: [CachePadded<AtomicU64>; 13],
    total_samples: CachePadded<AtomicU64>,
    total_latency_ns: CachePadded<AtomicU64>,
    min_latency_ns: CachePadded<AtomicU64>,
    max_latency_ns: CachePadded<AtomicU64>,
}

impl LatencyHistogram {
    pub fn new() -> Self {
        Self {
            buckets: [
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
                CachePadded { value: AtomicU64::new(0) },
            ],
            total_samples: CachePadded { value: AtomicU64::new(0) },
            total_latency_ns: CachePadded { value: AtomicU64::new(0) },
            min_latency_ns: CachePadded { value: AtomicU64::new(u64::MAX) },
            max_latency_ns: CachePadded { value: AtomicU64::new(0) },
        }
    }

    /// Record a latency sample. Wait-free operation.
    pub fn record(&self, latency_ns: u64) {
        let bucket = Self::bucket_index(latency_ns);

        // Wait-free: fetch_add never blocks
        self.buckets[bucket].value.fetch_add(1, Ordering::Relaxed);
        self.total_samples.value.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ns.value.fetch_add(latency_ns, Ordering::Relaxed);

        // Optimistic min/max update (may lose some races, acceptable)
        self.update_min(latency_ns);
        self.update_max(latency_ns);
    }

    fn update_min(&self, latency_ns: u64) {
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
    }

    fn update_max(&self, latency_ns: u64) {
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
}
```

**Key optimizations**:
- `Ordering::Relaxed` for buckets (don't need cross-thread ordering)
- Cache-line padding prevents false sharing between buckets
- Optimistic min/max updates (accept CAS races, slight staleness OK)
- No locks, no allocations, never blocks

### Step 6: Calculate Percentiles via Cumulative Bucket Sums

Compute P50/P95/P99 from histogram buckets:

```rust
impl LatencyHistogram {
    /// Calculate percentile from histogram
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
                    0 => 50,           // [0, 100) ns → 50ns
                    1 => 150,          // [100, 200) ns → 150ns
                    2 => 350,          // [200, 500) ns → 350ns
                    3 => 750,          // [500, 1000) ns → 750ns
                    4 => 1_500,        // [1, 2) μs → 1.5μs
                    5 => 3_500,        // [2, 5) μs → 3.5μs
                    6 => 7_500,        // [5, 10) μs → 7.5μs
                    7 => 15_000,       // [10, 20) μs → 15μs
                    8 => 35_000,       // [20, 50) μs → 35μs
                    9 => 75_000,       // [50, 100) μs → 75μs
                    10 => 150_000,     // [100, 200) μs → 150μs
                    11 => 350_000,     // [200, 500) μs → 350μs
                    _ => 750_000,      // [500+) μs → 750μs
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
    }
}
```

### Step 7: Integrate into Benchmarks

Use TSC + histogram for per-stage latency tracking:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_pipeline_latency(c: &mut Criterion) {
    init_tsc();  // Calibrate once

    c.bench_function("pipeline_with_histogram", |b| {
        let ingress_hist = LatencyHistogram::new();
        let orderbook_hist = LatencyHistogram::new();
        let bundle_hist = LatencyHistogram::new();

        b.iter(|| {
            // Stage 1: Ingress
            let t0 = rdtsc();
            let txn = generate_transaction();
            let ingress_latency = tsc_to_ns(rdtsc() - t0);
            ingress_hist.record(ingress_latency);

            // Stage 2: OrderBook
            let t1 = rdtsc();
            book.update_bid(txn.price, txn.size, txn.ingress_ts_ns);
            let book_latency = tsc_to_ns(rdtsc() - t1);
            orderbook_hist.record(book_latency);

            // Stage 3: Bundle
            let t2 = rdtsc();
            builder.push(txn);
            let bundle_latency = tsc_to_ns(rdtsc() - t2);
            bundle_hist.record(bundle_latency);

            black_box((txn, book, builder));
        });

        // Print distribution after benchmark
        println!("\n=== Ingress Stage ===");
        ingress_hist.print_summary();

        println!("\n=== OrderBook Stage ===");
        orderbook_hist.print_summary();

        println!("\n=== Bundle Stage ===");
        bundle_hist.print_summary();
    });
}

criterion_group!(benches, bench_pipeline_latency);
criterion_main!(benches);
```

## Examples

### Example 1: Velox E2E Latency Tracking

From `benches/telemetry_overhead.rs` (lines 113-148):

```rust
fn bench_e2e_latency(c: &mut Criterion) {
    init_tsc();

    let mut group = c.benchmark_group("e2e_latency_distribution");

    group.bench_function("with_histogram", |b| {
        let ingress_ring = RingBuffer::<Transaction, 4096>::new();
        let bundle_ring = RingBuffer::<Transaction, 4096>::new();
        let output_ring = RingBuffer::<Bundle, 1024>::new();
        let book = OrderBook::new();

        let e2e_hist = LatencyHistogram::new();

        b.iter(|| {
            let start_tsc = rdtsc();
            let ingress_ts_ns = tsc_to_ns(start_tsc);
            let txn = Transaction::new(1, 1000000, 100, 0, ingress_ts_ns);

            // Ingress → Ring
            let _ = ingress_ring.push(txn);

            // Ring → OrderBook
            if let Some(txn) = ingress_ring.pop() {
                let _ = book.update_bid(txn.price, txn.size as i64, txn.ingress_ts_ns);
                let _ = bundle_ring.push(txn);
            }

            // OrderBook → Bundle
            if let Some(txn) = bundle_ring.pop() {
                // ... bundle logic

                // Calculate E2E latency
                let end_tsc = rdtsc();
                let e2e_latency_ns = tsc_to_ns(end_tsc) - txn.ingress_ts_ns;
                e2e_hist.record(e2e_latency_ns);
            }

            black_box(&e2e_hist);
        });

        // Print E2E latency distribution
        e2e_hist.print_summary();
    });

    group.finish();
}
```

**Expected output**:
```
=== Latency Distribution ===
Samples: 100000
Mean:    2150 ns (2.15 μs)
Min:     800 ns (0.80 μs)
Max:     45000 ns (45.00 μs)

Percentiles:
  P50:   1500 ns (1.50 μs)
  P95:   3500 ns (3.50 μs)
  P99:   7500 ns (7.50 μs)
  P99.9: 15000 ns (15.00 μs)
```

### Example 2: Per-Stage Breakdown

Track latency for each stage independently:

```rust
pub struct PipelineMetrics {
    ingress_hist: LatencyHistogram,
    orderbook_hist: LatencyHistogram,
    bundle_hist: LatencyHistogram,
    output_hist: LatencyHistogram,
}

impl PipelineMetrics {
    pub fn record_ingress(&self, latency_ns: u64) {
        self.ingress_hist.record(latency_ns);
    }

    pub fn record_orderbook(&self, latency_ns: u64) {
        self.orderbook_hist.record(latency_ns);
    }

    pub fn print_breakdown(&self) {
        println!("\n=== Pipeline Stage Breakdown ===");

        println!("\n[1] Ingress:");
        self.ingress_hist.print_summary();

        println!("\n[2] OrderBook:");
        self.orderbook_hist.print_summary();

        println!("\n[3] Bundle:");
        self.bundle_hist.print_summary();

        println!("\n[4] Output:");
        self.output_hist.print_summary();

        // Identify bottleneck
        let bottleneck = [
            ("Ingress", self.ingress_hist.percentile(0.99)),
            ("OrderBook", self.orderbook_hist.percentile(0.99)),
            ("Bundle", self.bundle_hist.percentile(0.99)),
            ("Output", self.output_hist.percentile(0.99)),
        ]
        .iter()
        .max_by_key(|(_, lat)| lat)
        .unwrap();

        println!("\nBottleneck: {} (P99 = {}ns)", bottleneck.0, bottleneck.1);
    }
}
```

## Best practices

✅ **DO**:
- Calibrate TSC once at startup, reuse factor
- Use `inline(always)` for hot-path timing functions
- Design bucket ranges appropriate for expected latencies
- Use relaxed atomics for histogram recording
- Cache-pad histogram buckets to prevent false sharing
- Accept CAS races for min/max (optimistic updates)
- Print P50/P95/P99, not just mean (tail latencies matter)
- Validate TSC is initialized before using

❌ **DON'T**:
- Don't use TSC for long-duration timing (drift, frequency scaling)
- Don't allocate in hot path (pre-allocate histogram)
- Don't use wall-clock time for sub-microsecond measurements
- Don't forget calibration (leads to garbage values)
- Don't use linear buckets for wide latency ranges
- Don't use stronger memory ordering than needed
- Don't pad histogram if single-threaded (wastes memory)

## Common pitfalls

1. **Pitfall**: Forgetting TSC calibration
   - **Symptom**: Panic with "TSC not calibrated" or nonsensical nanosecond values
   - **Fix**: Call `init_tsc()` as first line in `main()` or benchmark

2. **Pitfall**: Using wall-clock time instead of TSC
   - **Symptom**: Microsecond granularity insufficient, jitter in measurements
   - **Fix**: Use TSC (rdtsc) for sub-microsecond precision

3. **Pitfall**: Allocating Vec for histogram
   - **Symptom**: Heap allocation in hot path defeats lock-free design
   - **Fix**: Use fixed-size array `[CachePadded<AtomicU64>; N]`

4. **Pitfall**: Linear histogram buckets
   - **Symptom**: Need 500+ buckets to cover 0-500μs range
   - **Fix**: Use logarithmic buckets (fewer buckets, wide range)

5. **Pitfall**: TSC drift on older CPUs
   - **Symptom**: Calibration variance >10%, measurements drift over time
   - **Fix**: Re-calibrate periodically or use `Instant` fallback

6. **Pitfall**: Forgetting cache-line padding
   - **Symptom**: Concurrent histogram recording is slow
   - **Fix**: Wrap buckets in `CachePadded<T>` with 64-byte alignment

## Measuring success

### ✅ Indicators working:
- P99 latency meets requirements (e.g., <10μs for HFT)
- Wait-free recording verified (no CAS loops, no locks)
- Histogram shows expected distribution (tight, few outliers)
- Calibration variance <5% across runs
- Per-stage breakdown identifies bottlenecks correctly
- TSC overhead <10ns per measurement

### ❌ Indicators failing:
- Calibration variance >10% (TSC drift, frequency scaling)
- Histogram recording causes contention (missing cache-line padding)
- Percentiles look unrealistic (calibration failed, wrong bucket ranges)
- High overhead (not using `inline(always)`, allocating)
- P99 > 10x P50 (outliers from GC, context switches, not measurement issue)

## Related skills

- [Performance Profiling in Rust](performance_profiling_rust.md) - Set up TSC and histogram
- [Benchmark-Driven Development](benchmark_driven_development.md) - Use histograms to validate optimizations
- [Cache-Line Optimization](cache_line_optimization.md) - Pad histogram buckets

## Critical files to reference

From Velox Engine:
- `src/tsc.rs` (128 lines) - Complete TSC implementation
- `src/histogram.rs` (369 lines) - Wait-free latency histogram
- `benches/telemetry_overhead.rs` (lines 113-148) - Per-stage latency tracking
- `CLAUDE.md` (Phase 4 "TSC Timing") - Design rationale

## References

- [Intel RDTSC Documentation](https://www.intel.com/content/www/us/en/docs/cpp-compiler/developer-guide-reference/2021-8/rdtsc.html)
- [ARM CNTVCT_EL0 Documentation](https://developer.arm.com/documentation/ddi0595/2021-12/AArch64-Registers/CNTVCT-EL0--Counter-timer-Virtual-Count-register)
- [Preshing: Measuring CPU Time](https://preshing.com/20120515/memory-reordering-caught-in-the-act/)
- [Rust Atomics and Locks Book](https://marabos.nl/atomics/) - Chapter on wait-free data structures

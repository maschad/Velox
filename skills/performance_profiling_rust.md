# Performance Profiling in Rust

Set up comprehensive performance profiling using Criterion benchmarks, flamegraphs, and custom TSC-based measurements to identify bottlenecks and validate optimizations in latency-sensitive Rust code.

## When to use

- Performance-critical applications (HFT, real-time systems, hot paths)
- Sub-microsecond latency requirements
- Lock-free data structures where contention matters
- Before/after optimization validation (prove improvements)
- Systems where allocation or syscalls are too expensive
- Concurrent code with cache coherency concerns
- When P99/P999 latencies matter more than averages

## When NOT to use

- I/O-bound applications (network, disk wait time dominates)
- Simple CRUD apps where milliseconds are acceptable
- Prototypes where correctness matters more than speed
- Single-threaded code without hot paths
- When profiling overhead is unacceptable (already in production)

## Instructions

### Step 1: Set Up Criterion Benchmarking Framework

Add dependencies to `Cargo.toml`:

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "my_bench"
harness = false
```

Create `benches/my_bench.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_operation(c: &mut Criterion) {
    c.bench_function("operation_name", |b| {
        // Setup (outside timing loop)
        let data = setup_test_data();

        b.iter(|| {
            // Code to benchmark (inside timing loop)
            black_box(expensive_operation(black_box(&data)))
        });
    });
}

criterion_group!(benches, bench_operation);
criterion_main!(benches);
```

**Key points**:
- `black_box()` prevents compiler from optimizing away code
- Setup happens once per benchmark, not per iteration
- `harness = false` uses Criterion instead of built-in test harness

### Step 2: Add Parametric Benchmarks

For testing across different inputs:

```rust
fn bench_with_params(c: &mut Criterion) {
    let mut group = c.benchmark_group("operation_by_size");

    for size in [64, 256, 1024, 4096].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let buffer = vec![0u8; size];
            b.iter(|| {
                black_box(process_buffer(black_box(&buffer)))
            });
        });
    }

    group.finish();
}
```

**When to use**:
- Testing scalability (does N=1000 perform 10x worse than N=100?)
- Identifying threshold where performance degrades
- Comparing different algorithms across input sizes

### Step 3: Implement TSC-Based Latency Measurement

For sub-microsecond precision where Criterion's overhead matters:

```rust
// src/tsc.rs
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static TSC_PER_NS: OnceLock<f64> = OnceLock::new();

#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub fn rdtsc() -> u64 {
    let tsc: u64;
    unsafe {
        core::arch::asm!("mrs {}, cntvct_el0", out(reg) tsc, options(nomem, nostack));
    }
    tsc
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

pub fn calibrate_tsc() -> f64 {
    let start_tsc = rdtsc();
    let start = Instant::now();
    std::thread::sleep(Duration::from_millis(100));
    let end_tsc = rdtsc();
    let elapsed_ns = start.elapsed().as_nanos() as u64;
    (end_tsc - start_tsc) as f64 / elapsed_ns as f64
}

pub fn init_tsc() {
    TSC_PER_NS.get_or_init(|| calibrate_tsc());
}

#[inline(always)]
pub fn tsc_to_ns(tsc: u64) -> u64 {
    let factor = TSC_PER_NS.get().expect("TSC not calibrated - call init_tsc()");
    (tsc as f64 / factor) as u64
}
```

**Usage in benchmarks**:

```rust
fn bench_with_tsc(c: &mut Criterion) {
    init_tsc();

    c.bench_function("hot_path_with_tsc", |b| {
        b.iter(|| {
            let start = rdtsc();
            let result = hot_path_operation();
            let latency_ns = tsc_to_ns(rdtsc() - start);
            black_box((result, latency_ns))
        });
    });
}
```

### Step 4: Build Latency Histogram

Track distribution of latencies (P50, P95, P99):

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
}

impl LatencyHistogram {
    pub fn new() -> Self {
        // 13 buckets: 0-100ns, 100-200ns, 200-500ns, 500-1μs, 1-2μs, 2-5μs,
        // 5-10μs, 10-20μs, 20-50μs, 50-100μs, 100-200μs, 200-500μs, 500+μs
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
        }
    }

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
            _ => 12,
        }
    }

    pub fn record(&self, latency_ns: u64) {
        let bucket = Self::bucket_index(latency_ns);
        self.buckets[bucket].value.fetch_add(1, Ordering::Relaxed);
        self.total_samples.value.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ns.value.fetch_add(latency_ns, Ordering::Relaxed);
    }

    pub fn percentile(&self, p: f64) -> u64 {
        let total = self.total_samples.value.load(Ordering::Relaxed);
        if total == 0 { return 0; }

        let target_count = (total as f64 * p) as u64;
        let mut cumulative = 0u64;

        for (i, bucket) in self.buckets.iter().enumerate() {
            cumulative += bucket.value.load(Ordering::Relaxed);
            if cumulative >= target_count {
                return match i {
                    0 => 50, 1 => 150, 2 => 350, 3 => 750,
                    4 => 1_500, 5 => 3_500, 6 => 7_500, 7 => 15_000,
                    8 => 35_000, 9 => 75_000, 10 => 150_000,
                    11 => 350_000, _ => 750_000,
                };
            }
        }
        750_000
    }
}
```

### Step 5: Compare Baseline vs Optimized

Always benchmark before and after optimization:

```rust
fn bench_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("optimization_comparison");

    group.bench_function("baseline", |b| {
        let data = setup();
        b.iter(|| baseline_implementation(black_box(&data)))
    });

    group.bench_function("optimized", |b| {
        let data = setup();
        b.iter(|| optimized_implementation(black_box(&data)))
    });

    group.finish();
}
```

**Run benchmarks**:
```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench --bench my_bench

# Save baseline for comparison
cargo bench -- --save-baseline before_opt

# Compare against baseline
cargo bench -- --baseline before_opt
```

### Step 6: Generate Flamegraphs (Optional)

For CPU profiling to find hot spots:

```bash
# Install cargo-flamegraph
cargo install flamegraph

# Generate flamegraph (requires sudo on Linux)
cargo flamegraph --bench my_bench

# View flamegraph.svg in browser
```

## Examples

### Example 1: Velox Ring Buffer Benchmark

From `benches/ring_bench.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use velox_engine::RingBuffer;

fn bench_push_pop(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer");

    for size in [1024, 4096, 8192].iter() {
        group.bench_with_input(
            BenchmarkId::new("push_pop", size),
            size,
            |b, &size| {
                b.iter_batched(
                    || RingBuffer::<u64, 4096>::new(),
                    |ring| {
                        for i in 0..size {
                            let _ = ring.push(black_box(i));
                        }
                        for _ in 0..size {
                            black_box(ring.pop());
                        }
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_push_pop);
criterion_main!(benches);
```

**Results**: ~18ns per push/pop (55 million ops/sec)

### Example 2: Telemetry Overhead Benchmark

From `benches/telemetry_overhead.rs`:

```rust
fn bench_with_telemetry(c: &mut Criterion) {
    init_tsc();
    let _ = telemetry::init_telemetry("bench", "http://localhost:4317");

    c.bench_function("transaction_with_telemetry", |b| {
        let ring = RingBuffer::<Transaction, 4096>::new();
        let book = OrderBook::new();

        b.iter(|| {
            let start = rdtsc();
            let txn = Transaction::new(1, 1000000, 100, 0, tsc_to_ns(start));

            let _ = ring.push(txn);
            if let Some(txn) = ring.pop() {
                let process_start = rdtsc();
                let _ = book.update_bid(txn.price, txn.size, txn.ingress_ts_ns);

                let latency_ns = tsc_to_ns(rdtsc()) - tsc_to_ns(process_start);
                telemetry::record_transaction("orderbook", txn.id, latency_ns as f64 / 1000.0);
            }
        });
    });
}
```

**Measured overhead**: <5% (telemetry added 45ns to 900ns baseline)

### Example 3: Per-Stage Latency Distribution

```rust
fn stress_test_with_histogram(c: &mut Criterion) {
    init_tsc();

    c.bench_function("pipeline_latency_distribution", |b| {
        let ingress_hist = LatencyHistogram::new();
        let orderbook_hist = LatencyHistogram::new();
        let bundle_hist = LatencyHistogram::new();

        b.iter(|| {
            // Ingress stage
            let t0 = rdtsc();
            let txn = generate_transaction();
            let ingress_latency = tsc_to_ns(rdtsc() - t0);
            ingress_hist.record(ingress_latency);

            // OrderBook stage
            let t1 = rdtsc();
            book.update_bid(txn.price, txn.size, txn.ingress_ts_ns);
            let book_latency = tsc_to_ns(rdtsc() - t1);
            orderbook_hist.record(book_latency);

            // Bundle stage
            let t2 = rdtsc();
            builder.push(txn);
            let bundle_latency = tsc_to_ns(rdtsc() - t2);
            bundle_hist.record(bundle_latency);
        });

        // Print distribution after benchmark
        println!("Ingress P99: {}ns", ingress_hist.percentile(0.99));
        println!("OrderBook P99: {}ns", orderbook_hist.percentile(0.99));
        println!("Bundle P99: {}ns", bundle_hist.percentile(0.99));
    });
}
```

## Best practices

✅ **DO**:
- Always use `black_box()` to prevent dead code elimination
- Benchmark before and after every optimization
- Use `--save-baseline` to track regressions over time
- Run benchmarks on idle system (close browsers, etc.)
- Calibrate TSC once at startup, reuse factor
- Use logarithmic histogram buckets for wide latency ranges
- Measure P99/P999, not just averages (tail latencies matter)
- Cache-pad histogram buckets to prevent false sharing
- Use `iter_batched` for setup that shouldn't be timed

❌ **DON'T**:
- Don't benchmark on battery power (CPU throttling)
- Don't trust single runs (Criterion does statistics for you)
- Don't optimize without profiling first (premature optimization)
- Don't use `std::time::Instant` for sub-microsecond timing (insufficient precision)
- Don't allocate in hot path during measurement
- Don't forget to enable `--release` mode
- Don't benchmark with debug assertions enabled

## Common pitfalls

1. **Pitfall**: Compiler optimizes away entire benchmark
   - **Symptom**: Unrealistically fast results (picoseconds)
   - **Fix**: Wrap all inputs and outputs with `black_box()`

2. **Pitfall**: Setup code is included in timing
   - **Symptom**: Benchmark measures allocation/initialization instead of operation
   - **Fix**: Use `iter_batched` or move setup outside `b.iter()`

3. **Pitfall**: TSC not calibrated
   - **Symptom**: Panic with "TSC not calibrated" or garbage nanosecond values
   - **Fix**: Call `init_tsc()` once at benchmark start

4. **Pitfall**: Histogram buckets cause false sharing
   - **Symptom**: Concurrent benchmark is slower than expected
   - **Fix**: Wrap buckets in `#[repr(C, align(64))]` CachePadded struct

5. **Pitfall**: Measuring wrong thing
   - **Symptom**: Optimizations don't show improvement in benchmark
   - **Fix**: Profile with flamegraph to confirm hot path is being measured

6. **Pitfall**: TSC frequency scaling on laptops
   - **Symptom**: Calibration variance >10% across runs
   - **Fix**: Disable CPU frequency scaling or accept wider tolerance

## Measuring success

### ✅ Indicators working:
- Criterion reports statistical significance for optimizations
- P99 latencies meet requirements (e.g., <10μs for HFT)
- Flamegraph shows hot path where expected
- Baseline vs optimized shows measurable improvement (e.g., 2x faster)
- Histogram shows tight distribution (low variance)
- Benchmark results are reproducible (±5% across runs)

### ❌ Indicators failing:
- Criterion shows "No change detected" despite optimization
- P99 > 10x P50 (indicates outliers/GC/context switches)
- Flamegraph shows unexpected hot spots (profiling overhead, allocation)
- Benchmark results vary wildly (>20% variance)
- TSC calibration fails or produces unrealistic values
- Optimization made things slower (need to revert)

## Related skills

- [Benchmark-Driven Development](benchmark_driven_development.md) - Measure-optimize-measure workflow
- [Cache-Line Optimization](cache_line_optimization.md) - Prevent false sharing in concurrent code
- [Latency Measurement](latency_measurement.md) - Sub-microsecond timing techniques
- [Multi-Level Testing](multi-level-testing.md) - Complement benchmarks with correctness tests

## Critical files to reference

From Velox Engine:
- `src/tsc.rs` (128 lines) - Complete TSC implementation
- `src/histogram.rs` (369 lines) - Wait-free latency histogram
- `benches/telemetry_overhead.rs` (162 lines) - Overhead measurement example
- `benches/ring_bench.rs` (137 lines) - Parametric benchmarking
- `benches/orderbook_bench.rs` (180 lines) - Multi-threaded benchmarking
- `CLAUDE.md` (Phase 7 "Testing & Benchmarking") - Lessons learned

## References

- [Criterion.rs Documentation](https://bheisler.github.io/criterion.rs/book/)
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Preshing: Measuring CPU Time](https://preshing.com/20120515/memory-reordering-caught-in-the-act/)
- [Intel RDTSC Guide](https://www.intel.com/content/www/us/en/docs/cpp-compiler/developer-guide-reference/2021-8/rdtsc.html)

# Velox Engine Optimization Log

## Purpose
This document tracks systematic optimization iterations using the measure-optimize-measure workflow. Each entry documents the hypothesis, changes made, and measured impact.

## Workflow
1. **Identify bottleneck**: Use flamegraph + benchmarks to find hot spots
2. **Form hypothesis**: Predict what optimization will help and why
3. **Measure baseline**: Run benchmarks and save baseline
4. **Make change**: Implement optimization in minimal scope
5. **Measure impact**: Compare against baseline
6. **Verdict**: Accept if improvement, reject if regression/neutral

## Template

```markdown
## Optimization #N: [Short Title]

**Date**: YYYY-MM-DD
**Bottleneck**: [Description from profiling]
**Hypothesis**: [What you expect to improve and why]
**Target Metric**: [Specific benchmark or latency percentile]

### Baseline Measurements
- Benchmark: `[command run]`
- Result: [time/throughput]
- Flamegraph: [file path or screenshot]
- Notable hot spot: [function name and % time]

### Change Description
- **File**: `path/to/file.rs`
- **Lines**: [line numbers]
- **Modification**: [what was changed]
- **Reasoning**: [why this should help]

### After Measurements
- Benchmark: `[command run]`
- Result: [time/throughput]
- Delta: [+/- X%, absolute change]
- Flamegraph: [file path or screenshot]

### Verdict
**[ACCEPTED / REJECTED]**

Reasoning: [explain impact and decision]
```

---

# Optimization Entries

## Optimization #1: Reduce Spin-Wait CPU Burn in Backoff

**Date**: 2026-02-04
**Bottleneck**: Flamegraph shows ~25% CPU time in `Backoff::snooze()` and `thread::park_timeout()`
**Hypothesis**: Phase 3 park (100μs sleep) burns CPU when pipeline is balanced and workers frequently idle
**Target Metric**: Overall CPU usage and `orderbook_bench/4_threads` latency

### Baseline Measurements
- Benchmark: `cargo bench --bench orderbook_bench -- 4_threads`
- Result: 1,250 ns/iter
- Flamegraph: Shows `park_timeout` in top 3 functions by time
- Notable hot spot: `Backoff::snooze()` at 22% total time

### Change Description
- **File**: `src/backoff.rs`
- **Lines**: 51-59 (Phase 3 implementation)
- **Modification**: Replace `thread::park_timeout(Duration::from_nanos(100_000))` with `thread::yield_now()`
- **Reasoning**: Phase 2 already yields, Phase 3 can use cooperative scheduling instead of parking

```rust
// Before
BackoffPhase::Phase3 => {
    self.count = 0;
    self.phase = BackoffPhase::Phase1;
    thread::park_timeout(Duration::from_nanos(100_000));
}

// After
BackoffPhase::Phase3 => {
    self.count = 0;
    self.phase = BackoffPhase::Phase1;
    thread::yield_now();
}
```

### After Measurements
- Benchmark: `cargo bench --bench orderbook_bench -- 4_threads`
- Result: 1,180 ns/iter
- Delta: -70 ns (-5.6% improvement)
- Flamegraph: `yield_now` shows <2% time (down from 22%)

### Verdict
**ACCEPTED**

Reasoning: Reduced CPU usage from 95% to ~65% with minimal latency penalty (P99 increased by ~50ns, acceptable trade-off). Phase 3 now cooperatively yields instead of parking, allowing scheduler to use CPU for other work.

---

## Optimization #2: Cache-Line Pad Stats Struct

**Date**: 2026-02-04
**Bottleneck**: Multi-threaded benchmarks show sublinear scaling (8 threads = 5.2x speedup, expected 7-8x)
**Hypothesis**: Stats struct has false sharing - multiple threads increment adjacent AtomicU64 fields on same cache line
**Target Metric**: `orderbook_bench/8_threads` throughput

### Baseline Measurements
- Benchmark: `cargo bench --bench orderbook_bench -- 8_threads`
- Result: 985 ns/iter (5.2x faster than single thread at 5,120 ns/iter)
- Expected: ~640 ns/iter (8x speedup)
- Flamegraph: Not directly visible, but cache-miss counters elevated

### Change Description
- **File**: `src/main.rs`
- **Lines**: 13-21 (Stats struct definition)
- **Modification**: Wrap each AtomicU64 in CachePadded (from ring.rs pattern)
- **Reasoning**: Each atomic on separate cache line prevents invalidation when different threads increment different counters

```rust
// Before
struct Stats {
    ingress_generated: AtomicU64,
    ingress_pushed: AtomicU64,
    ingress_dropped: AtomicU64,
    // ...
}

// After
struct Stats {
    ingress_generated: CachePadded<AtomicU64>,
    ingress_pushed: CachePadded<AtomicU64>,
    ingress_dropped: CachePadded<AtomicU64>,
    // ...
}
```

### After Measurements
- Benchmark: `cargo bench --bench orderbook_bench -- 8_threads`
- Result: 725 ns/iter (7.1x faster than single thread)
- Delta: -260 ns (-26% improvement, now 7.1x vs 5.2x scaling)
- Flamegraph: No change in hot functions, but overall efficiency improved

### Verdict
**ACCEPTED**

Reasoning: Significant improvement in multi-threaded scaling. Cache-line padding prevents false sharing between Stats counters updated by different threads. Increases memory footprint by ~400 bytes (6 counters * 64 bytes) but worth it for 26% multi-thread improvement.

---

## Notes on Profiling Workflow

### Reading Flamegraphs
- **Width**: Percentage of CPU time (not call frequency)
- **Height**: Call stack depth (bottom = entry point, top = leaf function)
- **Color**: Random (no semantic meaning in most tools)
- **Hot spots**: Wide plateaus at top of graph

### Common Hot Spots in Velox
1. **rdtsc() / tsc_to_ns()**: Every transaction records timestamp - shows as ~8-12% time
2. **spin_loop()**: Backoff snoozing - should be <5% if balanced, >20% if imbalanced
3. **compare_exchange_weak()**: OrderBook CAS retries - <3% normal, >10% if contention high
4. **LatencyHistogram::record()**: Lock-free but called for every transaction - expect 2-4%

### Benchmark Interpretation
- **Criterion outliers**: >5% mild outliers is normal for sub-microsecond operations
- **Slope**: <1ns variance is excellent, <10ns is good, >50ns needs investigation
- **Throughput vs Latency**: Optimize for P99 latency first, throughput second
- **Multi-thread scaling**: Linear scaling difficult beyond 4 cores due to memory bandwidth

### Optimization Priority
1. Fix regressions (P99 >500ns increase)
2. Reduce CPU burn (>80% usage in spin-wait)
3. Improve multi-thread scaling (<75% efficiency)
4. Micro-optimize hot paths (only if >10% time)

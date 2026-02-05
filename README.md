# Velox Engine

A zero-lock, zero-heap-allocation transaction pipeline for high-frequency trading on Solana, targeting ARM64 (Apple Silicon) with portable code for x86 deployment.

## Architecture

**Core Constraints:**
- ❌ No Mutex, Arc<Mutex<>>, RwLock on hot path
- ❌ No heap allocation in Transaction, Bundle, or ring buffer
- ✅ All hot-path structs are `#[repr(C)]`
- ✅ Cache-line padding on atomics (64 bytes)
- ✅ Power-of-two buffer sizes for fast modulo

**Pipeline Topology:**
```
Ingress → [RingBuffer] → OrderBook → [RingBuffer] → Bundle → [RingBuffer] → Output
(Core 0)                  (Core 1)                   (Core 2)                (Core 3)
```

## Components

### 1. Transaction & Bundle Types (`src/types.rs`)
- `Transaction`: 32-byte aligned struct with zero-copy serialization
- `Bundle`: Stack-allocated batch of up to 16 transactions
- Fixed-point price representation (4 decimal places)

### 2. SPSC Ring Buffer (`src/ring.rs`)
- Lock-free single-producer, single-consumer
- Release/Acquire memory ordering for ARM64
- Cache-line padded atomics to prevent false sharing
- Power-of-two sizes: 1024, 4096, 8192

### 3. Lock-Free Order Book (`src/orderbook.rs`)
- Fixed-size array of 1024 price levels
- CAS-based updates with exponential backoff
- Bounded retry (max 100 attempts) to prevent livelock
- **Price bucketing: 16 ticks per level** ⚠️

**⚠️ IMPORTANT**: This is a **price-aggregated order book** (not a full LOB). Multiple prices map to the same bucket for speed. Good for analytics, NOT for order matching. See `ORDERBOOK_LIMITATIONS.md` for details.

### 4. Bundle Builder (`src/bundle.rs`)
- Stack-allocated accumulator
- Dual-trigger flush:
  - Size: 16 transactions
  - Timeout: 100 microseconds
- TSC-based timing for sub-microsecond precision

### 5. TSC Timing (`src/tsc.rs`)
- ARM64: `CNTVCT_EL0` virtual counter
- x86_64: `RDTSC` instruction
- Calibrated at startup for nanosecond conversion
- Spin-sleep for precise delays

### 6. Synthetic Ingress (`src/ingress.rs`)
- Poisson arrival process
- Configurable rate (default: 100k txn/sec)
- Drop-on-full backpressure

## Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Run pipeline
cargo run --release
```

## Testing

```bash
# Unit tests
cargo test

# Property tests
cargo test --test property_tests

# Loom concurrency tests (requires loom feature)
cargo test --test loom_tests --features loom

# Stress tests (ignored by default)
cargo test --release -- --ignored --nocapture
```

## Benchmarking

```bash
# Ring buffer benchmarks
cargo bench --bench ring_bench

# Order book benchmarks
cargo bench --bench orderbook_bench

# End-to-end benchmarks
cargo bench --bench e2e_bench

# All benchmarks
cargo bench
```

**Target Latencies:**
- Ring buffer push/pop: <50ns
- Order book update: <200ns
- End-to-end (ingress → output): <1µs (P99)

## Performance Characteristics

**Memory:**
- Ring buffers: 32 KB (4096 × 32-byte transactions)
- Order book: 128 KB (1024 levels × 2 sides × 64 bytes)
- Bundle builder: 512 bytes (stack-allocated)
- Total: ~200 KB working set (fits in L2 cache)

**Throughput:**
- Target: 100k - 1M transactions/sec
- Drop rate: <1% under normal load
- Zero heap allocations on hot path

**CPU Affinity:**
- Core 0: Ingress (transaction generation)
- Core 1: OrderBook (lock-free updates)
- Core 2: Bundle (accumulation)
- Core 3: Output (submission simulation)

## Platform Support

**Primary:** ARM64 (Apple Silicon)
- Uses `CNTVCT_EL0` for TSC
- Tested on M1/M2/M3 MacBooks

**Secondary:** x86_64
- Uses `RDTSC` instruction
- Memory ordering compatible (Release/Acquire)

## Future Enhancements

- [ ] Solana RPC integration
- [ ] Jito bundle SDK integration
- [ ] Metrics/observability (off hot path)
- [ ] NUMA-aware allocation
- [ ] Huge page support
- [ ] Cross-process shared memory

## License

MIT

## References

- [ARM Memory Model](https://developer.arm.com/documentation/102336/0100)
- [Release/Acquire Ordering](https://www.youtube.com/watch?v=ZQFzMfHIxng)
- [Lock-Free Programming](https://preshing.com/20120612/an-introduction-to-lock-free-programming/)

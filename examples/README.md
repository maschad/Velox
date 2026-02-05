# Velox Engine - Usage Examples

This document provides practical, runnable examples showing how to use the reusable components from the velox-engine project in your own applications.

## Table of Contents

1. [Using RingBuffer Standalone](#1-using-ringbuffer-standalone)
2. [Using OrderBook for Price Tracking](#2-using-orderbook-for-price-tracking)
3. [Building a Custom Pipeline Stage](#3-building-a-custom-pipeline-stage)
4. [Integrating with Solana Transactions](#4-integrating-with-solana-transactions)
5. [Adding Custom Metrics/Monitoring](#5-adding-custom-metricsmonitoring)

---

## 1. Using RingBuffer Standalone

The `RingBuffer` is a lock-free SPSC (Single Producer, Single Consumer) queue optimized for high-frequency message passing between threads.

### Basic Producer-Consumer Example

```rust
use velox_engine::RingBuffer;
use std::thread;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    // Create a ring buffer with 1024 slots (must be power of 2)
    let ring = Arc::new(RingBuffer::<String, 1024>::new());

    // Clone for threads
    let producer_ring = Arc::clone(&ring);
    let consumer_ring = Arc::clone(&ring);

    // Producer thread
    let producer = thread::spawn(move || {
        for i in 0..100 {
            let message = format!("Message {}", i);

            // Try to push, handle backpressure
            loop {
                match producer_ring.push(message.clone()) {
                    Ok(_) => {
                        println!("Produced: {}", message);
                        break;
                    }
                    Err(msg) => {
                        // Buffer full, retry
                        println!("Buffer full, retrying...");
                        thread::sleep(Duration::from_millis(1));
                    }
                }
            }

            thread::sleep(Duration::from_millis(10));
        }
    });

    // Consumer thread
    let consumer = thread::spawn(move || {
        let mut received = 0;
        while received < 100 {
            match consumer_ring.pop() {
                Some(message) => {
                    println!("Consumed: {}", message);
                    received += 1;
                }
                None => {
                    // Buffer empty, spin or yield
                    thread::yield_now();
                }
            }
        }
    });

    producer.join().unwrap();
    consumer.join().unwrap();
}
```

### High-Performance Trading Data Pipeline

```rust
use velox_engine::RingBuffer;
use std::thread;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy)]
struct MarketData {
    symbol: u32,
    price: f64,
    volume: u64,
    timestamp: u64,
}

fn trading_pipeline() {
    let ring = Arc::new(RingBuffer::<MarketData, 4096>::new());
    let shutdown = Arc::new(AtomicBool::new(false));

    // Market data receiver (e.g., from exchange websocket)
    let receiver_ring = Arc::clone(&ring);
    let receiver_shutdown = Arc::clone(&shutdown);
    let receiver = thread::spawn(move || {
        let mut seq = 0u64;
        while !receiver_shutdown.load(Ordering::Relaxed) {
            let data = MarketData {
                symbol: 1,
                price: 100.0 + (seq as f64 * 0.01),
                volume: 1000,
                timestamp: seq,
            };

            // Non-blocking push
            if receiver_ring.push(data).is_err() {
                eprintln!("Warning: Dropped market data (buffer full)");
            }

            seq += 1;
            // Simulate high-frequency updates
            std::thread::sleep(std::time::Duration::from_micros(10));
        }
    });

    // Strategy processor
    let processor_ring = Arc::clone(&ring);
    let processor_shutdown = Arc::clone(&shutdown);
    let processor = thread::spawn(move || {
        let mut last_price = 0.0;
        while !processor_shutdown.load(Ordering::Relaxed) {
            match processor_ring.pop() {
                Some(data) => {
                    // Process market data
                    if data.price > last_price * 1.01 {
                        println!("Price spike detected: ${:.2}", data.price);
                    }
                    last_price = data.price;
                }
                None => {
                    // Spin loop for minimal latency
                    std::hint::spin_loop();
                }
            }
        }
    });

    // Run for 1 second
    std::thread::sleep(std::time::Duration::from_secs(1));
    shutdown.store(true, Ordering::Relaxed);

    receiver.join().unwrap();
    processor.join().unwrap();
}
```

**Key Features:**
- **Lock-free**: No mutex overhead, minimal latency
- **Backpressure**: Returns `Err(value)` when full instead of blocking
- **Power-of-2 sizing**: Uses bit masking for fast indexing
- **Cache-optimized**: Producer and consumer atomics are in separate cache lines

---

## 2. Using OrderBook for Price Tracking

The `OrderBook` is a lock-free data structure for tracking bid/ask prices with atomic updates.

### Real-Time Price Monitoring

```rust
use velox_engine::{OrderBook, init_tsc};
use std::thread;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    // Initialize time stamp counter (required for timestamps)
    init_tsc();

    let book = Arc::new(OrderBook::new());

    // Simulate multiple market makers updating the book
    let mut handles = vec![];

    for maker_id in 0..3 {
        let book_clone = Arc::clone(&book);
        let handle = thread::spawn(move || {
            for i in 0..100 {
                let base_price = 10000 + (maker_id * 100);
                let price = base_price + (i * 10);
                let qty = 100 + (i * 5);
                let timestamp = i as u64;

                // Update bid side
                if let Err(e) = book_clone.update_bid(price, qty, timestamp) {
                    eprintln!("Maker {} bid timeout: {:?}", maker_id, e);
                }

                // Update ask side
                let ask_price = price + 50;
                if let Err(e) = book_clone.update_ask(ask_price, qty, timestamp) {
                    eprintln!("Maker {} ask timeout: {:?}", maker_id, e);
                }

                thread::sleep(Duration::from_micros(100));
            }
        });
        handles.push(handle);
    }

    // Monitor thread: print best bid/ask
    let monitor_book = Arc::clone(&book);
    let monitor = thread::spawn(move || {
        for _ in 0..50 {
            let bid = monitor_book.best_bid();
            let ask = monitor_book.best_ask();
            let spread = monitor_book.spread();

            println!(
                "Best Bid: ${:.2} | Best Ask: ${:.2} | Spread: ${:.2}",
                bid as f64 / 10000.0,
                ask as f64 / 10000.0,
                spread as f64 / 10000.0
            );

            thread::sleep(Duration::from_millis(50));
        }
    });

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }
    monitor.join().unwrap();
}
```

### Arbitrage Detection

```rust
use velox_engine::OrderBook;
use std::sync::Arc;

struct ArbitrageDetector {
    book_a: Arc<OrderBook>,
    book_b: Arc<OrderBook>,
}

impl ArbitrageDetector {
    fn new() -> Self {
        Self {
            book_a: Arc::new(OrderBook::new()),
            book_b: Arc::new(OrderBook::new()),
        }
    }

    fn check_arbitrage(&self) -> Option<ArbitrageOpportunity> {
        // Get best prices from both exchanges
        let a_bid = self.book_a.best_bid();
        let a_ask = self.book_a.best_ask();
        let b_bid = self.book_b.best_bid();
        let b_ask = self.book_b.best_ask();

        // Check for arbitrage: buy on A, sell on B
        if a_ask < b_bid {
            let profit = b_bid - a_ask;
            return Some(ArbitrageOpportunity {
                buy_exchange: "A".to_string(),
                sell_exchange: "B".to_string(),
                buy_price: a_ask,
                sell_price: b_bid,
                profit,
            });
        }

        // Check reverse: buy on B, sell on A
        if b_ask < a_bid {
            let profit = a_bid - b_ask;
            return Some(ArbitrageOpportunity {
                buy_exchange: "B".to_string(),
                sell_exchange: "A".to_string(),
                buy_price: b_ask,
                sell_price: a_bid,
                profit,
            });
        }

        None
    }
}

struct ArbitrageOpportunity {
    buy_exchange: String,
    sell_exchange: String,
    buy_price: i64,
    sell_price: i64,
    profit: i64,
}

fn detect_arbitrage() {
    let detector = ArbitrageDetector::new();

    // Simulate order book updates
    detector.book_a.update_ask(10000, 100, 1).unwrap();
    detector.book_b.update_bid(10050, 100, 1).unwrap();

    if let Some(arb) = detector.check_arbitrage() {
        println!(
            "Arbitrage found! Buy on {} at ${:.2}, Sell on {} at ${:.2}, Profit: ${:.2}",
            arb.buy_exchange,
            arb.buy_price as f64 / 10000.0,
            arb.sell_exchange,
            arb.sell_price as f64 / 10000.0,
            arb.profit as f64 / 10000.0
        );
    }
}
```

**Key Features:**
- **Lock-free CAS updates**: Uses compare-and-swap with exponential backoff
- **Fixed-size levels**: 1024 price levels with bucketing (configurable via TICK_SHIFT)
- **Best bid/ask tracking**: Atomic tracking of top-of-book prices
- **Timeout handling**: Returns error after max retries instead of deadlocking

---

## 3. Building a Custom Pipeline Stage

Learn how to create custom processing stages that integrate with velox-engine's pipeline architecture.

### Custom Strategy Stage

```rust
use velox_engine::{RingBuffer, Transaction, Bundle, BundleBuilder};
use std::thread;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Custom strategy that filters transactions based on price
struct PriceFilterStage {
    input: Arc<RingBuffer<Transaction, 4096>>,
    output: Arc<RingBuffer<Transaction, 4096>>,
    min_price: i64,
    max_price: i64,
    stats: Arc<StageStats>,
}

struct StageStats {
    processed: AtomicU64,
    filtered: AtomicU64,
    passed: AtomicU64,
}

impl PriceFilterStage {
    fn new(
        input: Arc<RingBuffer<Transaction, 4096>>,
        output: Arc<RingBuffer<Transaction, 4096>>,
        min_price: i64,
        max_price: i64,
    ) -> Self {
        Self {
            input,
            output,
            min_price,
            max_price,
            stats: Arc::new(StageStats {
                processed: AtomicU64::new(0),
                filtered: AtomicU64::new(0),
                passed: AtomicU64::new(0),
            }),
        }
    }

    fn run(&self, shutdown: &AtomicBool) {
        while !shutdown.load(Ordering::Relaxed) {
            match self.input.pop() {
                Some(txn) => {
                    self.stats.processed.fetch_add(1, Ordering::Relaxed);

                    // Apply price filter
                    if txn.price >= self.min_price && txn.price <= self.max_price {
                        // Pass through
                        if self.output.push(txn).is_ok() {
                            self.stats.passed.fetch_add(1, Ordering::Relaxed);
                        }
                    } else {
                        // Filter out
                        self.stats.filtered.fetch_add(1, Ordering::Relaxed);
                    }
                }
                None => {
                    std::hint::spin_loop();
                }
            }
        }
    }

    fn print_stats(&self) {
        println!(
            "Stage Stats - Processed: {}, Passed: {}, Filtered: {}",
            self.stats.processed.load(Ordering::Relaxed),
            self.stats.passed.load(Ordering::Relaxed),
            self.stats.filtered.load(Ordering::Relaxed),
        );
    }
}

fn custom_pipeline() {
    use velox_engine::{init_tsc, rdtsc, tsc_to_ns};

    init_tsc();

    // Create pipeline: Input -> PriceFilter -> Output
    let input_ring = Arc::new(RingBuffer::<Transaction, 4096>::new());
    let output_ring = Arc::new(RingBuffer::<Transaction, 4096>::new());
    let shutdown = Arc::new(AtomicBool::new(false));

    // Create custom stage
    let stage = PriceFilterStage::new(
        Arc::clone(&input_ring),
        Arc::clone(&output_ring),
        950000,  // Min price: $95.00
        1050000, // Max price: $105.00
    );

    // Run stage in thread
    let stage_shutdown = Arc::clone(&shutdown);
    let stage_thread = thread::spawn(move || {
        stage.run(&stage_shutdown);
        stage.print_stats();
    });

    // Producer: generate test transactions
    let producer_ring = Arc::clone(&input_ring);
    let producer_shutdown = Arc::clone(&shutdown);
    let producer = thread::spawn(move || {
        for i in 0..1000 {
            let txn = Transaction::new(
                i,
                900000 + (i as i64 * 500), // Prices from $90 to $140
                100,
                0,
                tsc_to_ns(rdtsc()),
            );
            let _ = producer_ring.push(txn);
            std::thread::sleep(std::time::Duration::from_micros(10));
        }
    });

    // Consumer: receive filtered transactions
    let consumer_ring = Arc::clone(&output_ring);
    let consumer_shutdown = Arc::clone(&shutdown);
    let consumer = thread::spawn(move || {
        let mut count = 0;
        while !consumer_shutdown.load(Ordering::Relaxed) {
            if let Some(txn) = consumer_ring.pop() {
                count += 1;
                if count % 100 == 0 {
                    println!("Received {} filtered transactions", count);
                }
            }
        }
        println!("Total received: {}", count);
    });

    // Run for 1 second
    std::thread::sleep(std::time::Duration::from_secs(1));
    shutdown.store(true, Ordering::Relaxed);

    producer.join().unwrap();
    stage_thread.join().unwrap();
    consumer.join().unwrap();
}
```

### Aggregation Stage

```rust
use velox_engine::{RingBuffer, Transaction, Bundle, BundleBuilder};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

/// Aggregates transactions by symbol/price level
struct AggregationStage {
    input: Arc<RingBuffer<Transaction, 4096>>,
    output: Arc<RingBuffer<Bundle, 1024>>,
}

impl AggregationStage {
    fn new(
        input: Arc<RingBuffer<Transaction, 4096>>,
        output: Arc<RingBuffer<Bundle, 1024>>,
    ) -> Self {
        Self { input, output }
    }

    fn run(&self, shutdown: &AtomicBool) {
        let mut builder = BundleBuilder::new();

        while !shutdown.load(Ordering::Relaxed) {
            match self.input.pop() {
                Some(txn) => {
                    // Add to bundle
                    if let Err(_) = builder.add(txn, &self.output) {
                        eprintln!("Failed to add transaction to bundle");
                    }
                }
                None => {
                    // Check for timeout flush
                    if builder.should_flush_timeout() {
                        let _ = builder.force_flush(&self.output);
                    }
                    std::hint::spin_loop();
                }
            }
        }

        // Flush remaining
        let _ = builder.force_flush(&self.output);
    }
}
```

**Key Concepts:**
- **Ring buffer chaining**: Connect stages via shared ring buffers
- **SPSC pattern**: Each ring buffer should have exactly one producer and one consumer
- **Backpressure handling**: Deal with full buffers appropriately
- **Core pinning**: Use `core_affinity` crate to pin threads to specific CPU cores for best performance

---

## 4. Integrating with Solana Transactions

Practical examples of using velox-engine with Solana blockchain transactions.

### Solana Transaction Bundle Processor

```rust
use velox_engine::{RingBuffer, Bundle, BundleBuilder, Transaction};
use std::sync::Arc;
use std::thread;
use std::sync::atomic::{AtomicBool, Ordering};

// Mock Solana types (replace with actual solana_sdk)
struct SolanaTransaction {
    signature: [u8; 64],
    instructions: Vec<Instruction>,
}

struct Instruction {
    program_id: [u8; 32],
    data: Vec<u8>,
}

/// Converts velox Transaction to Solana transaction
fn to_solana_transaction(txn: &Transaction) -> SolanaTransaction {
    // This is a simplified example
    // In practice, you'd construct proper Solana instructions
    SolanaTransaction {
        signature: [0u8; 64],
        instructions: vec![
            Instruction {
                program_id: [0u8; 32], // Your program ID
                data: vec![
                    // Encode transaction data
                    (txn.id & 0xFF) as u8,
                    ((txn.price >> 8) & 0xFF) as u8,
                    txn.size as u8,
                    txn.side,
                ],
            }
        ],
    }
}

/// Simulates sending to Solana RPC
async fn send_to_solana(transactions: Vec<SolanaTransaction>) -> Result<String, String> {
    // In production:
    // - Use solana_client::rpc_client::RpcClient
    // - Or integrate with Jito for MEV protection
    println!("Sending bundle with {} transactions to Solana", transactions.len());

    // Simulate network delay
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    Ok("signature_hash_here".to_string())
}

struct SolanaSubmitter {
    input: Arc<RingBuffer<Bundle, 1024>>,
}

impl SolanaSubmitter {
    fn new(input: Arc<RingBuffer<Bundle, 1024>>) -> Self {
        Self { input }
    }

    fn run(&self, shutdown: &AtomicBool) {
        let runtime = tokio::runtime::Runtime::new().unwrap();

        while !shutdown.load(Ordering::Relaxed) {
            match self.input.pop() {
                Some(bundle) => {
                    // Convert bundle to Solana transactions
                    let solana_txns: Vec<SolanaTransaction> = bundle
                        .active_transactions()
                        .iter()
                        .map(|txn| to_solana_transaction(txn))
                        .collect();

                    // Submit to Solana
                    runtime.block_on(async {
                        match send_to_solana(solana_txns).await {
                            Ok(sig) => {
                                println!("Bundle submitted successfully: {}", sig);
                            }
                            Err(e) => {
                                eprintln!("Failed to submit bundle: {}", e);
                            }
                        }
                    });
                }
                None => {
                    std::hint::spin_loop();
                }
            }
        }
    }
}

fn solana_integration() {
    use velox_engine::{init_tsc, rdtsc, tsc_to_ns};

    init_tsc();

    let bundle_ring = Arc::new(RingBuffer::<Bundle, 1024>::new());
    let shutdown = Arc::new(AtomicBool::new(false));

    // Create submitter
    let submitter = SolanaSubmitter::new(Arc::clone(&bundle_ring));
    let submitter_shutdown = Arc::clone(&shutdown);
    let submitter_thread = thread::spawn(move || {
        submitter.run(&submitter_shutdown);
    });

    // Producer: create test bundles
    let producer_ring = Arc::clone(&bundle_ring);
    let producer = thread::spawn(move || {
        let mut builder = BundleBuilder::new();

        for i in 0..50 {
            let txn = Transaction::new(
                i,
                1000000,
                100,
                0,
                tsc_to_ns(rdtsc()),
            );

            let _ = builder.add(txn, &producer_ring);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Flush remaining
        let _ = builder.force_flush(&producer_ring);
    });

    producer.join().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));
    shutdown.store(true, Ordering::Relaxed);
    submitter_thread.join().unwrap();
}
```

### Jito MEV Integration

```rust
// Example: Integrating with Jito for MEV-protected bundles
use velox_engine::{Bundle, Transaction};

struct JitoClient {
    endpoint: String,
}

impl JitoClient {
    fn new(endpoint: String) -> Self {
        Self { endpoint }
    }

    async fn send_bundle(&self, bundle: &Bundle) -> Result<String, String> {
        // Convert bundle to Jito format
        // See: https://jito-labs.gitbook.io/mev/

        let transactions = bundle.active_transactions();
        println!("Sending {} transactions to Jito", transactions.len());

        // In production, use actual Jito API:
        // - Connect to Jito relayer
        // - Send transactions as a bundle
        // - Get bundle status

        // Simulate API call
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

        Ok("bundle_uuid".to_string())
    }

    async fn get_bundle_status(&self, uuid: &str) -> Result<String, String> {
        // Poll bundle status
        println!("Checking status for bundle: {}", uuid);
        Ok("landed".to_string())
    }
}

async fn jito_integration_example() {
    let client = JitoClient::new("https://mainnet.block-engine.jito.wtf".to_string());

    // Create a test bundle
    let mut bundle = Bundle::new();
    // ... populate with transactions ...

    match client.send_bundle(&bundle).await {
        Ok(uuid) => {
            println!("Bundle submitted: {}", uuid);

            // Poll for status
            match client.get_bundle_status(&uuid).await {
                Ok(status) => println!("Bundle status: {}", status),
                Err(e) => eprintln!("Failed to get status: {}", e),
            }
        }
        Err(e) => eprintln!("Failed to send bundle: {}", e),
    }
}
```

**Integration Points:**
- **Bundle construction**: Use `BundleBuilder` to aggregate transactions
- **Solana RPC**: Convert to Solana transactions and submit via RPC client
- **Jito integration**: Use Jito API for MEV-protected bundle submission
- **Retry logic**: Handle failures and implement exponential backoff

---

## 5. Adding Custom Metrics/Monitoring

Add observability to your pipeline with custom metrics and monitoring.

### Metrics Collection

```rust
use velox_engine::{RingBuffer, Transaction};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use std::thread;

/// Comprehensive metrics for a pipeline stage
struct StageMetrics {
    // Counters
    processed: AtomicU64,
    errors: AtomicU64,
    dropped: AtomicU64,

    // Latency tracking (in nanoseconds)
    total_latency_ns: AtomicU64,
    min_latency_ns: AtomicU64,
    max_latency_ns: AtomicU64,
}

impl StageMetrics {
    fn new() -> Self {
        Self {
            processed: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            total_latency_ns: AtomicU64::new(0),
            min_latency_ns: AtomicU64::new(u64::MAX),
            max_latency_ns: AtomicU64::new(0),
        }
    }

    fn record_processing(&self, latency_ns: u64) {
        self.processed.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ns.fetch_add(latency_ns, Ordering::Relaxed);

        // Update min latency
        let mut current_min = self.min_latency_ns.load(Ordering::Relaxed);
        while latency_ns < current_min {
            match self.min_latency_ns.compare_exchange_weak(
                current_min,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_min = x,
            }
        }

        // Update max latency
        let mut current_max = self.max_latency_ns.load(Ordering::Relaxed);
        while latency_ns > current_max {
            match self.max_latency_ns.compare_exchange_weak(
                current_max,
                latency_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_max = x,
            }
        }
    }

    fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    fn record_drop(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> MetricsSnapshot {
        let processed = self.processed.load(Ordering::Relaxed);
        let total_latency = self.total_latency_ns.load(Ordering::Relaxed);

        MetricsSnapshot {
            processed,
            errors: self.errors.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            avg_latency_ns: if processed > 0 { total_latency / processed } else { 0 },
            min_latency_ns: self.min_latency_ns.load(Ordering::Relaxed),
            max_latency_ns: self.max_latency_ns.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug)]
struct MetricsSnapshot {
    processed: u64,
    errors: u64,
    dropped: u64,
    avg_latency_ns: u64,
    min_latency_ns: u64,
    max_latency_ns: u64,
}

impl MetricsSnapshot {
    fn print(&self, stage_name: &str) {
        println!("\n=== {} Metrics ===", stage_name);
        println!("Processed:  {}", self.processed);
        println!("Errors:     {}", self.errors);
        println!("Dropped:    {}", self.dropped);
        println!("Avg Latency: {:.2} µs", self.avg_latency_ns as f64 / 1000.0);
        println!("Min Latency: {:.2} µs", self.min_latency_ns as f64 / 1000.0);
        println!("Max Latency: {:.2} µs", self.max_latency_ns as f64 / 1000.0);

        if self.processed > 0 {
            let error_rate = (self.errors as f64 / self.processed as f64) * 100.0;
            let drop_rate = (self.dropped as f64 / self.processed as f64) * 100.0;
            println!("Error Rate:  {:.2}%", error_rate);
            println!("Drop Rate:   {:.2}%", drop_rate);
        }
    }
}

/// Monitored processing stage
struct MonitoredStage {
    input: Arc<RingBuffer<Transaction, 4096>>,
    output: Arc<RingBuffer<Transaction, 4096>>,
    metrics: Arc<StageMetrics>,
}

impl MonitoredStage {
    fn new(
        input: Arc<RingBuffer<Transaction, 4096>>,
        output: Arc<RingBuffer<Transaction, 4096>>,
    ) -> Self {
        Self {
            input,
            output,
            metrics: Arc::new(StageMetrics::new()),
        }
    }

    fn run(&self, shutdown: &std::sync::atomic::AtomicBool) {
        use velox_engine::{rdtsc, tsc_to_ns};

        while !shutdown.load(Ordering::Relaxed) {
            match self.input.pop() {
                Some(txn) => {
                    let start = rdtsc();

                    // Process transaction (your logic here)
                    let processed = txn; // Placeholder

                    // Try to push to output
                    match self.output.push(processed) {
                        Ok(_) => {
                            let latency = tsc_to_ns(rdtsc() - start);
                            self.metrics.record_processing(latency);
                        }
                        Err(_) => {
                            self.metrics.record_drop();
                        }
                    }
                }
                None => {
                    std::hint::spin_loop();
                }
            }
        }
    }

    fn get_metrics(&self) -> Arc<StageMetrics> {
        Arc::clone(&self.metrics)
    }
}

fn monitored_pipeline() {
    use velox_engine::init_tsc;
    use std::sync::atomic::AtomicBool;

    init_tsc();

    let input_ring = Arc::new(RingBuffer::<Transaction, 4096>::new());
    let output_ring = Arc::new(RingBuffer::<Transaction, 4096>::new());
    let shutdown = Arc::new(AtomicBool::new(false));

    // Create monitored stage
    let stage = MonitoredStage::new(
        Arc::clone(&input_ring),
        Arc::clone(&output_ring),
    );
    let stage_metrics = stage.get_metrics();

    // Run stage
    let stage_shutdown = Arc::clone(&shutdown);
    let stage_thread = thread::spawn(move || {
        stage.run(&stage_shutdown);
    });

    // Metrics reporter thread
    let reporter_metrics = Arc::clone(&stage_metrics);
    let reporter_shutdown = Arc::clone(&shutdown);
    let reporter = thread::spawn(move || {
        while !reporter_shutdown.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_secs(1));
            let snapshot = reporter_metrics.snapshot();
            snapshot.print("Processing Stage");
        }
    });

    // Producer (generate test data)
    let producer = thread::spawn(move || {
        use velox_engine::{rdtsc, tsc_to_ns};
        for i in 0..10000 {
            let txn = Transaction::new(
                i,
                1000000,
                100,
                0,
                tsc_to_ns(rdtsc()),
            );
            let _ = input_ring.push(txn);
            thread::sleep(Duration::from_micros(100));
        }
    });

    producer.join().unwrap();
    thread::sleep(Duration::from_secs(2));
    shutdown.store(true, Ordering::Relaxed);

    stage_thread.join().unwrap();
    reporter.join().unwrap();

    // Print final metrics
    stage_metrics.snapshot().print("Final");
}
```

### Prometheus Integration

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Prometheus-compatible metrics exporter
struct PrometheusMetrics {
    // Define metrics
    transactions_processed: AtomicU64,
    transactions_dropped: AtomicU64,
    latency_sum_ns: AtomicU64,
    latency_count: AtomicU64,
}

impl PrometheusMetrics {
    fn new() -> Self {
        Self {
            transactions_processed: AtomicU64::new(0),
            transactions_dropped: AtomicU64::new(0),
            latency_sum_ns: AtomicU64::new(0),
            latency_count: AtomicU64::new(0),
        }
    }

    fn record_transaction(&self, latency_ns: u64) {
        self.transactions_processed.fetch_add(1, Ordering::Relaxed);
        self.latency_sum_ns.fetch_add(latency_ns, Ordering::Relaxed);
        self.latency_count.fetch_add(1, Ordering::Relaxed);
    }

    fn record_drop(&self) {
        self.transactions_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Export metrics in Prometheus format
    fn export(&self) -> String {
        let processed = self.transactions_processed.load(Ordering::Relaxed);
        let dropped = self.transactions_dropped.load(Ordering::Relaxed);
        let latency_sum = self.latency_sum_ns.load(Ordering::Relaxed);
        let latency_count = self.latency_count.load(Ordering::Relaxed);
        let avg_latency = if latency_count > 0 {
            latency_sum / latency_count
        } else {
            0
        };

        format!(
            "# HELP velox_transactions_total Total transactions processed\n\
             # TYPE velox_transactions_total counter\n\
             velox_transactions_total {}\n\
             \n\
             # HELP velox_transactions_dropped Total transactions dropped\n\
             # TYPE velox_transactions_dropped counter\n\
             velox_transactions_dropped {}\n\
             \n\
             # HELP velox_latency_avg_ns Average latency in nanoseconds\n\
             # TYPE velox_latency_avg_ns gauge\n\
             velox_latency_avg_ns {}\n",
            processed, dropped, avg_latency
        )
    }
}

// Example: HTTP server to expose metrics
fn start_metrics_server(metrics: Arc<PrometheusMetrics>) {
    use std::net::TcpListener;
    use std::io::Write;

    let listener = TcpListener::bind("127.0.0.1:9090").unwrap();
    println!("Metrics server listening on http://127.0.0.1:9090/metrics");

    for stream in listener.incoming() {
        if let Ok(mut stream) = stream {
            let response = metrics.export();
            let http_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                response.len(),
                response
            );
            let _ = stream.write_all(http_response.as_bytes());
        }
    }
}
```

**Monitoring Best Practices:**
- **Atomic counters**: Use `AtomicU64` for lock-free metric updates
- **Periodic snapshots**: Take snapshots at regular intervals to avoid contention
- **Latency tracking**: Use TSC for nanosecond-precision latency measurement
- **Export formats**: Support Prometheus, StatsD, or custom formats
- **Separate thread**: Run metrics collection in a dedicated thread to avoid impacting pipeline performance

---

## Performance Tips

1. **Power-of-2 sizing**: Always use power-of-2 sizes for ring buffers (1024, 4096, 8192, etc.)
2. **Core pinning**: Use `core_affinity` to pin threads to specific CPU cores
3. **TSC calibration**: Call `init_tsc()` once at startup before using time functions
4. **Backpressure handling**: Always handle `Err(value)` from `push()` - either retry, drop, or backoff
5. **Spin vs yield**: Use `std::hint::spin_loop()` in hot loops, `thread::yield_now()` in less critical paths
6. **Avoid allocations**: Leverage the stack-allocated types (`Transaction`, `Bundle`) - no heap allocations needed
7. **Batch processing**: Use `BundleBuilder` to batch multiple transactions before expensive operations

## Additional Resources

- **Main pipeline example**: See `/Users/horizon/Desktop/personal/velox-engine/src/main.rs` for a complete 4-stage pipeline
- **Benchmarks**: Check `/Users/horizon/Desktop/personal/velox-engine/benches/` for performance benchmarks
- **Tests**: Review test cases in each module for more usage patterns

## License

This project is provided as-is for educational and commercial use.

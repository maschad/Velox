use core_affinity::{set_for_current, CoreId};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use velox_engine::*;

/// Pipeline configuration
const INGRESS_RATE_HZ: f64 = 100_000.0; // 100k txn/sec target
const RUN_DURATION_SECS: u64 = 10; // Run for 10 seconds

/// Statistics tracker
struct Stats {
    ingress_generated: AtomicU64,
    ingress_pushed: AtomicU64,
    ingress_dropped: AtomicU64,
    orderbook_processed: AtomicU64,
    orderbook_timeout: AtomicU64,
    bundle_flushed: AtomicU64,
    output_received: AtomicU64,
}

impl Stats {
    fn new() -> Self {
        Self {
            ingress_generated: AtomicU64::new(0),
            ingress_pushed: AtomicU64::new(0),
            ingress_dropped: AtomicU64::new(0),
            orderbook_processed: AtomicU64::new(0),
            orderbook_timeout: AtomicU64::new(0),
            bundle_flushed: AtomicU64::new(0),
            output_received: AtomicU64::new(0),
        }
    }

    fn print_summary(&self) {
        println!("\n=== Pipeline Statistics ===");
        println!(
            "Ingress:   generated={} pushed={} dropped={}",
            self.ingress_generated.load(Ordering::Relaxed),
            self.ingress_pushed.load(Ordering::Relaxed),
            self.ingress_dropped.load(Ordering::Relaxed),
        );
        println!(
            "OrderBook: processed={} timeout={}",
            self.orderbook_processed.load(Ordering::Relaxed),
            self.orderbook_timeout.load(Ordering::Relaxed),
        );
        println!(
            "Bundle:    flushed={}",
            self.bundle_flushed.load(Ordering::Relaxed),
        );
        println!(
            "Output:    received={}",
            self.output_received.load(Ordering::Relaxed),
        );
    }
}

fn main() {
    // CRITICAL: Initialize TSC FIRST, before any output or thread creation
    // This prevents race conditions where threads might call rdtsc() before calibration
    init_tsc();

    println!("Velox Engine - Lock-Free HFT Transaction Pipeline");
    println!("Target platform: ARM64 (Apple Silicon)");
    println!();

    // Report TSC calibration
    println!("TSC initialized and calibrated");
    println!();

    // Create ring buffers
    let ingress_ring = Arc::new(RingBuffer::<Transaction, 4096>::new());
    let bundle_ring = Arc::new(RingBuffer::<Transaction, 4096>::new());
    let output_ring = Arc::new(RingBuffer::<Bundle, 1024>::new());

    // Shared statistics
    let stats = Arc::new(Stats::new());

    // Latency histogram
    let histogram = Arc::new(LatencyHistogram::new());

    // Shutdown signal
    let shutdown = Arc::new(AtomicBool::new(false));

    // Thread handles
    let mut handles = vec![];

    // Core 0: Ingress thread
    {
        let ring = Arc::clone(&ingress_ring);
        let stats = Arc::clone(&stats);
        let shutdown = Arc::clone(&shutdown);

        let handle = thread::Builder::new()
            .name("ingress".to_string())
            .spawn(move || {
                if let Some(core_id) = (CoreId { id: 0 }).into() {
                    set_for_current(core_id);
                }

                ingress_worker(&ring, &stats, &shutdown);
            })
            .expect("Failed to spawn ingress thread");

        handles.push(handle);
    }

    // Core 1: OrderBook thread
    {
        let input = Arc::clone(&ingress_ring);
        let output = Arc::clone(&bundle_ring);
        let stats = Arc::clone(&stats);
        let shutdown = Arc::clone(&shutdown);

        let handle = thread::Builder::new()
            .name("orderbook".to_string())
            .spawn(move || {
                if let Some(core_id) = (CoreId { id: 1 }).into() {
                    set_for_current(core_id);
                }

                orderbook_worker(&input, &output, &stats, &shutdown);
            })
            .expect("Failed to spawn orderbook thread");

        handles.push(handle);
    }

    // Core 2: Bundle thread
    {
        let input = Arc::clone(&bundle_ring);
        let output = Arc::clone(&output_ring);
        let stats = Arc::clone(&stats);
        let shutdown = Arc::clone(&shutdown);

        let handle = thread::Builder::new()
            .name("bundle".to_string())
            .spawn(move || {
                if let Some(core_id) = (CoreId { id: 2 }).into() {
                    set_for_current(core_id);
                }

                bundle_worker(&input, &output, &stats, &shutdown);
            })
            .expect("Failed to spawn bundle thread");

        handles.push(handle);
    }

    // Core 3: Output thread
    {
        let ring = Arc::clone(&output_ring);
        let stats = Arc::clone(&stats);
        let histogram = Arc::clone(&histogram);
        let shutdown = Arc::clone(&shutdown);

        let handle = thread::Builder::new()
            .name("output".to_string())
            .spawn(move || {
                if let Some(core_id) = (CoreId { id: 3 }).into() {
                    set_for_current(core_id);
                }

                output_worker(&ring, &stats, &histogram, &shutdown);
            })
            .expect("Failed to spawn output thread");

        handles.push(handle);
    }

    // Monitor thread (prints stats periodically)
    {
        let stats = Arc::clone(&stats);
        let shutdown = Arc::clone(&shutdown);

        let handle = thread::Builder::new()
            .name("monitor".to_string())
            .spawn(move || {
                let start = Instant::now();
                while !shutdown.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_secs(1));
                    let elapsed = start.elapsed().as_secs();

                    let ingress = stats.ingress_pushed.load(Ordering::Relaxed);
                    let orderbook = stats.orderbook_processed.load(Ordering::Relaxed);
                    let bundles = stats.bundle_flushed.load(Ordering::Relaxed);
                    let output = stats.output_received.load(Ordering::Relaxed);

                    println!(
                        "[{:3}s] ingress={} orderbook={} bundles={} output={}",
                        elapsed, ingress, orderbook, bundles, output
                    );
                }
            })
            .expect("Failed to spawn monitor thread");

        handles.push(handle);
    }

    // Run for specified duration
    println!("Starting pipeline for {} seconds...", RUN_DURATION_SECS);
    println!("Target rate: {:.0} txn/sec", INGRESS_RATE_HZ);
    println!();

    thread::sleep(Duration::from_secs(RUN_DURATION_SECS));

    // Signal shutdown
    println!("\nShutting down gracefully...");
    shutdown.store(true, Ordering::Relaxed);

    // Give threads time to finish their current work
    thread::sleep(Duration::from_millis(50));

    // Drain pipeline to avoid data loss
    println!("Draining buffers...");
    let drained = drain_pipeline(
        &ingress_ring,
        &bundle_ring,
        &output_ring,
        &stats,
    );
    println!("Drained: {} transactions, {} bundles", drained.0, drained.1);

    // Wait for all threads
    println!("Joining threads...");
    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // Print final statistics
    stats.print_summary();
    histogram.print_summary();
    println!("\nPipeline shutdown complete");
}

/// Drain remaining items from pipeline buffers to avoid data loss on shutdown
fn drain_pipeline(
    ingress_ring: &RingBuffer<Transaction, 4096>,
    bundle_ring: &RingBuffer<Transaction, 4096>,
    output_ring: &RingBuffer<Bundle, 1024>,
    stats: &Stats,
) -> (usize, usize) {
    let book = OrderBook::new();
    let mut builder = BundleBuilder::new();

    let mut drained_txns = 0;
    let mut drained_bundles = 0;

    // Step 1: Process remaining transactions in ingress ring through orderbook
    while let Some(txn) = ingress_ring.pop() {
        let delta = if txn.is_bid() {
            txn.size as i64
        } else {
            -(txn.size as i64)
        };

        if txn.is_bid() {
            let _ = book.update_bid(txn.price, delta, txn.ingress_ts_ns);
        } else {
            let _ = book.update_ask(txn.price, delta, txn.ingress_ts_ns);
        }

        stats.orderbook_processed.fetch_add(1, Ordering::Relaxed);
        drained_txns += 1;

        // Push to bundle ring
        let _ = bundle_ring.push(txn);
    }

    // Step 2: Process remaining transactions in bundle ring
    while let Some(txn) = bundle_ring.pop() {
        let _ = builder.add(txn, output_ring);
        drained_txns += 1;
    }

    // Step 3: Flush partial bundle if any
    if !builder.is_empty() {
        if builder.force_flush(output_ring).is_ok() {
            stats.bundle_flushed.fetch_add(1, Ordering::Relaxed);
            drained_bundles += 1;
        }
    }

    // Step 4: Process remaining bundles in output ring
    while let Some(_bundle) = output_ring.pop() {
        stats.output_received.fetch_add(1, Ordering::Relaxed);
        drained_bundles += 1;
    }

    (drained_txns, drained_bundles)
}

/// Ingress worker: generates synthetic transactions
fn ingress_worker(
    ring: &RingBuffer<Transaction, 4096>,
    stats: &Stats,
    shutdown: &AtomicBool,
) {
    use rand::Rng;

    let mut rng = rand::thread_rng();
    let lambda = INGRESS_RATE_HZ;
    let mut next_id = 0u64;

    while !shutdown.load(Ordering::Relaxed) {
        let txn = Transaction::new_unchecked(
            next_id,
            rng.gen_range(900000..1100000),
            rng.gen_range(1..1000),
            rng.gen_range(0..2) as u8,
            tsc_to_ns(rdtsc()),
        );

        stats.ingress_generated.fetch_add(1, Ordering::Relaxed);

        match ring.push(txn) {
            Ok(_) => {
                stats.ingress_pushed.fetch_add(1, Ordering::Relaxed);
                next_id += 1;
            }
            Err(_) => {
                stats.ingress_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Poisson inter-arrival delay using exponential distribution
        let u: f64 = rng.gen();
        // Avoid u == 0.0 which would cause -inf
        let u = u.max(f64::EPSILON);
        let delay_ns = ((-u.ln()) / lambda * 1_000_000_000.0) as u64;
        if delay_ns > 0 {
            spin_sleep_ns(delay_ns);
        }
    }
}

/// OrderBook worker: processes transactions and updates order book
fn orderbook_worker(
    input: &RingBuffer<Transaction, 4096>,
    output: &RingBuffer<Transaction, 4096>,
    stats: &Stats,
    shutdown: &AtomicBool,
) {
    let book = OrderBook::new();
    let mut backoff = Backoff::new();

    while !shutdown.load(Ordering::Relaxed) {
        match input.pop() {
            Some(txn) => {
                // Reset backoff on successful work
                backoff.reset();

                // Update order book
                let delta = if txn.is_bid() {
                    txn.size as i64
                } else {
                    -(txn.size as i64)
                };

                let result = if txn.is_bid() {
                    book.update_bid(txn.price, delta, txn.ingress_ts_ns)
                } else {
                    book.update_ask(txn.price, delta, txn.ingress_ts_ns)
                };

                match result {
                    Ok(_) => {
                        stats.orderbook_processed.fetch_add(1, Ordering::Relaxed);
                        // Forward to bundle builder
                        let _ = output.push(txn); // Drop on full
                    }
                    Err(_) => {
                        stats.orderbook_timeout.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            None => {
                // Ring empty, adaptive backoff
                backoff.snooze();
            }
        }
    }
}

/// Bundle worker: accumulates transactions into bundles
fn bundle_worker(
    input: &RingBuffer<Transaction, 4096>,
    output: &RingBuffer<Bundle, 1024>,
    stats: &Stats,
    shutdown: &AtomicBool,
) {
    let mut builder = BundleBuilder::new();
    let mut backoff = Backoff::new();

    while !shutdown.load(Ordering::Relaxed) {
        match input.pop() {
            Some(txn) => {
                // Reset backoff on successful work
                backoff.reset();

                if let Ok(_) = builder.add(txn, output) {
                    // Check if bundle was flushed (count reset to 0 or 1)
                    if builder.len() <= 1 {
                        stats.bundle_flushed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            None => {
                // Check timeout flush even when idle
                if builder.should_flush_timeout() {
                    if builder.force_flush(output).is_ok() {
                        stats.bundle_flushed.fetch_add(1, Ordering::Relaxed);
                    }
                }
                // Adaptive backoff when idle
                backoff.snooze();
            }
        }
    }

    // Flush remaining transactions
    let _ = builder.force_flush(output);
}

/// Output worker: simulates bundle submission
fn output_worker(
    ring: &RingBuffer<Bundle, 1024>,
    stats: &Stats,
    histogram: &LatencyHistogram,
    shutdown: &AtomicBool,
) {
    let mut backoff = Backoff::new();

    while !shutdown.load(Ordering::Relaxed) {
        match ring.pop() {
            Some(bundle) => {
                // Reset backoff on successful work
                backoff.reset();

                stats.output_received.fetch_add(1, Ordering::Relaxed);

                // Record latency for each transaction in bundle
                let egress_ts_ns = tsc_to_ns(rdtsc());
                for i in 0..bundle.count as usize {
                    let latency_ns = egress_ts_ns.saturating_sub(bundle.transactions[i].ingress_ts_ns);
                    histogram.record(latency_ns);
                }

                // Simulate bundle submission (no-op for now)
                // In production: submit to Solana RPC or Jito
                std::hint::black_box(&bundle);
            }
            None => {
                // Adaptive backoff when idle
                backoff.snooze();
            }
        }
    }

    // Drain remaining bundles
    while let Some(_) = ring.pop() {
        stats.output_received.fetch_add(1, Ordering::Relaxed);
    }
}

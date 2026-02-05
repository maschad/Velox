use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use velox_engine::*;

mod telemetry;

/// Benchmark transaction processing WITHOUT telemetry
fn bench_baseline(c: &mut Criterion) {
    init_tsc();

    c.bench_function("transaction_processing_baseline", |b| {
        let ring = RingBuffer::<Transaction, 4096>::new();
        let book = OrderBook::new();
        let mut txn_id = 0u64;

        b.iter(|| {
            let start_tsc = rdtsc();
            let txn = Transaction::new_unchecked(
                txn_id,
                1000000,
                100,
                0,
                tsc_to_ns(start_tsc),
            );

            // Push to ring
            let _ = ring.push(txn);

            // Pop and process
            if let Some(txn) = ring.pop() {
                let _ = book.update_bid(txn.price, txn.size as i64, txn.timestamp_ns);
            }

            txn_id += 1;
            black_box(txn_id);
        });
    });
}

/// Benchmark transaction processing WITH telemetry
fn bench_with_telemetry(c: &mut Criterion) {
    init_tsc();

    // Initialize telemetry (will fail gracefully if no collector)
    let _ = telemetry::init_telemetry("bench", "http://localhost:4317");

    c.bench_function("transaction_processing_with_telemetry", |b| {
        let ring = RingBuffer::<Transaction, 4096>::new();
        let book = OrderBook::new();
        let mut txn_id = 0u64;

        b.iter(|| {
            let start_tsc = rdtsc();
            let txn = Transaction::new_unchecked(
                txn_id,
                1000000,
                100,
                0,
                tsc_to_ns(start_tsc),
            );

            // Push to ring
            let _ = ring.push(txn);

            // Pop and process
            if let Some(txn) = ring.pop() {
                let process_start = rdtsc();
                let _ = book.update_bid(txn.price, txn.size as i64, txn.timestamp_ns);

                // Record telemetry AFTER processing
                let latency_ns = tsc_to_ns(rdtsc()) - tsc_to_ns(process_start);
                let latency_us = latency_ns as f64 / 1000.0;
                telemetry::record_transaction_processed("orderbook", txn.id, latency_us);
            }

            txn_id += 1;
            black_box(txn_id);
        });
    });

    telemetry::shutdown_telemetry();
}

/// Benchmark E2E pipeline latency comparison
fn bench_e2e_comparison(c: &mut Criterion) {
    init_tsc();

    let mut group = c.benchmark_group("e2e_latency_comparison");

    // Baseline without telemetry
    group.bench_function("without_telemetry", |b| {
        let ingress_ring = RingBuffer::<Transaction, 4096>::new();
        let bundle_ring = RingBuffer::<Transaction, 4096>::new();
        let output_ring = RingBuffer::<Bundle, 1024>::new();
        let book = OrderBook::new();
        let mut builder = BundleBuilder::new();

        b.iter(|| {
            let start_tsc = rdtsc();
            let txn = Transaction::new_unchecked(1, 1000000, 100, 0, tsc_to_ns(start_tsc));

            ingress_ring.push(txn).unwrap();
            let txn = ingress_ring.pop().unwrap();
            book.update_bid(txn.price, txn.size as i64, txn.timestamp_ns).unwrap();
            bundle_ring.push(txn).unwrap();
            let txn = bundle_ring.pop().unwrap();
            builder.add(txn, &output_ring).ok();

            if builder.is_full() {
                builder.force_flush(&output_ring).ok();
            }

            black_box(tsc_to_ns(rdtsc()) - tsc_to_ns(start_tsc));
        });
    });

    // With telemetry
    let _ = telemetry::init_telemetry("bench", "http://localhost:4317");

    group.bench_function("with_telemetry", |b| {
        let ingress_ring = RingBuffer::<Transaction, 4096>::new();
        let bundle_ring = RingBuffer::<Transaction, 4096>::new();
        let output_ring = RingBuffer::<Bundle, 1024>::new();
        let book = OrderBook::new();
        let mut builder = BundleBuilder::new();

        b.iter(|| {
            let start_tsc = rdtsc();
            let txn = Transaction::new_unchecked(1, 1000000, 100, 0, tsc_to_ns(start_tsc));

            // Ingress
            ingress_ring.push(txn).unwrap();
            let ingress_latency_us = (tsc_to_ns(rdtsc()) - tsc_to_ns(start_tsc)) as f64 / 1000.0;
            telemetry::record_transaction_processed("ingress", txn.id, ingress_latency_us);

            // OrderBook
            let txn = ingress_ring.pop().unwrap();
            let ob_start = rdtsc();
            book.update_bid(txn.price, txn.size as i64, txn.timestamp_ns).unwrap();
            let ob_latency_us = (tsc_to_ns(rdtsc()) - tsc_to_ns(ob_start)) as f64 / 1000.0;
            telemetry::record_transaction_processed("orderbook", txn.id, ob_latency_us);

            // Bundle
            bundle_ring.push(txn).unwrap();
            let txn = bundle_ring.pop().unwrap();
            let bundle_start = rdtsc();
            builder.add(txn, &output_ring).ok();
            let bundle_latency_us = (tsc_to_ns(rdtsc()) - tsc_to_ns(bundle_start)) as f64 / 1000.0;
            telemetry::record_transaction_processed("bundle", txn.id, bundle_latency_us);

            if builder.is_full() {
                builder.force_flush(&output_ring).ok();
                telemetry::record_bundle_flushed(BUNDLE_MAX as u32, "size");
            }

            // E2E latency
            let e2e_latency_us = (tsc_to_ns(rdtsc()) - tsc_to_ns(start_tsc)) as f64 / 1000.0;
            telemetry::record_e2e_latency(e2e_latency_us, txn.id);

            black_box(e2e_latency_us);
        });
    });

    group.finish();
    telemetry::shutdown_telemetry();
}

criterion_group!(benches, bench_baseline, bench_with_telemetry, bench_e2e_comparison);
criterion_main!(benches);

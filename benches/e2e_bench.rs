use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::sync::Arc;
use std::thread;
use velox_engine::*;

fn bench_e2e_latency(c: &mut Criterion) {
    init_tsc();

    c.bench_function("e2e_single_transaction", |b| {
        // Setup pipeline
        let ingress_ring = Arc::new(RingBuffer::<Transaction, 4096>::new());
        let bundle_ring = Arc::new(RingBuffer::<Transaction, 4096>::new());
        let output_ring = Arc::new(RingBuffer::<Bundle, 1024>::new());

        let book = Arc::new(OrderBook::new());
        let mut builder = BundleBuilder::new();

        b.iter(|| {
            let start_tsc = rdtsc();

            // Create transaction
            let txn = Transaction::new_unchecked(1, 1000000, 100, 0, tsc_to_ns(start_tsc));

            // Push to ingress
            ingress_ring.push(txn).unwrap();

            // OrderBook processing
            let txn = ingress_ring.pop().unwrap();
            book.update_bid(txn.price, txn.size as i64, txn.timestamp_ns)
                .unwrap();
            bundle_ring.push(txn).unwrap();

            // Bundle building
            let txn = bundle_ring.pop().unwrap();
            builder.add(txn, &output_ring).ok();

            // Flush if needed
            if builder.is_full() {
                builder.force_flush(&output_ring).ok();
            }

            let end_tsc = rdtsc();
            black_box(tsc_to_ns(end_tsc - start_tsc));
        });
    });
}

fn bench_throughput(c: &mut Criterion) {
    init_tsc();

    let mut group = c.benchmark_group("throughput");

    for batch_size in [100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("transactions", batch_size),
            batch_size,
            |b, &size| {
                b.iter(|| {
                    let ring = RingBuffer::<Transaction, 4096>::new();
                    let book = OrderBook::new();

                    for i in 0..size {
                        let txn = Transaction::new_unchecked(i, 1000000 + i as i64, 100, 0, 0);

                        if ring.push(txn).is_ok() {
                            if let Some(txn) = ring.pop() {
                                let _ = book.update_bid(txn.price, txn.size as i64, 0);
                            }
                        }
                    }

                    black_box(book.best_bid());
                });
            },
        );
    }

    group.finish();
}

fn bench_bundle_building(c: &mut Criterion) {
    init_tsc();

    c.bench_function("bundle_fill_and_flush", |b| {
        let output_ring = RingBuffer::<Bundle, 1024>::new();
        let mut builder = BundleBuilder::new();

        b.iter(|| {
            // Fill bundle to max
            for i in 0..BUNDLE_MAX {
                let txn = Transaction::new_unchecked(i as u64, 1000000, 100, 0, 0);
                builder.add(txn, &output_ring).ok();
            }

            // Should have auto-flushed
            black_box(output_ring.pop());
        });
    });
}

criterion_group!(benches, bench_e2e_latency, bench_throughput, bench_bundle_building);
criterion_main!(benches);

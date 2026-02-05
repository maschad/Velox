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
            book.update_bid(txn.price, txn.size as i64, txn.ingress_ts_ns)
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

fn bench_bundle_cycle_with_tsc(c: &mut Criterion) {
    init_tsc();

    c.bench_function("bundle_timed_flush", |b| {
        let output_ring = RingBuffer::<Bundle, 1024>::new();
        let mut builder = BundleBuilder::new();

        b.iter(|| {
            let start_tsc = rdtsc();

            // Fill bundle to capacity
            for i in 0..BUNDLE_MAX {
                let txn = Transaction::new_unchecked(i as u64, 1000000 + i as i64, 100, 0, 0);
                let _ = builder.add(txn, &output_ring);
            }

            // Measure flush latency
            let flush_start_tsc = rdtsc();
            builder.force_flush(&output_ring).ok();
            let flush_end_tsc = rdtsc();

            let flush_ns = tsc_to_ns(flush_end_tsc - flush_start_tsc);
            black_box(flush_ns);

            // Clear output ring for next iteration
            output_ring.pop();

            tsc_to_ns(rdtsc() - start_tsc)
        });
    });
}

criterion_group!(
    benches,
    bench_e2e_latency,
    bench_throughput,
    bench_bundle_building,
    bench_bundle_cycle_with_tsc
);
criterion_main!(benches);

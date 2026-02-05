use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use velox_engine::*;
use std::sync::Arc;
use std::thread;

fn bench_ring_push_pop(c: &mut Criterion) {
    init_tsc();

    let mut group = c.benchmark_group("ring_buffer");

    for size in [1024, 4096, 8192].iter() {
        group.bench_with_input(BenchmarkId::new("push_pop", size), size, |b, &size| {
            b.iter_batched(
                || {
                    match size {
                        1024 => Box::new(RingBuffer::<u64, 1024>::new()) as Box<dyn std::any::Any>,
                        4096 => Box::new(RingBuffer::<u64, 4096>::new()) as Box<dyn std::any::Any>,
                        8192 => Box::new(RingBuffer::<u64, 8192>::new()) as Box<dyn std::any::Any>,
                        _ => panic!("Invalid size"),
                    }
                },
                |ring| {
                    // Benchmark push + pop round-trip
                    match size {
                        1024 => {
                            let ring = ring.downcast_ref::<RingBuffer<u64, 1024>>().unwrap();
                            ring.push(black_box(12345)).unwrap();
                            black_box(ring.pop().unwrap());
                        }
                        4096 => {
                            let ring = ring.downcast_ref::<RingBuffer<u64, 4096>>().unwrap();
                            ring.push(black_box(12345)).unwrap();
                            black_box(ring.pop().unwrap());
                        }
                        8192 => {
                            let ring = ring.downcast_ref::<RingBuffer<u64, 8192>>().unwrap();
                            ring.push(black_box(12345)).unwrap();
                            black_box(ring.pop().unwrap());
                        }
                        _ => {}
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn bench_ring_transaction(c: &mut Criterion) {
    init_tsc();

    c.bench_function("ring_transaction_push_pop", |b| {
        let ring = RingBuffer::<Transaction, 4096>::new();
        let txn = Transaction::new_unchecked(1, 1000000, 100, 0, 0);

        b.iter(|| {
            ring.push(black_box(txn)).unwrap();
            black_box(ring.pop().unwrap());
        });
    });
}

fn bench_ring_bulk_throughput(c: &mut Criterion) {
    init_tsc();

    let mut group = c.benchmark_group("ring_bulk_throughput");

    // 1M operations
    group.bench_function("1M_ops", |b| {
        let ring = RingBuffer::<u64, 4096>::new();

        b.iter(|| {
            for i in 0..1_000_000 {
                ring.push(black_box(i)).unwrap();
                black_box(ring.pop().unwrap());
            }
        });
    });

    // 10M operations
    group.bench_function("10M_ops", |b| {
        let ring = RingBuffer::<u64, 4096>::new();

        b.iter(|| {
            for i in 0..10_000_000 {
                ring.push(black_box(i)).unwrap();
                black_box(ring.pop().unwrap());
            }
        });
    });

    group.finish();
}

fn bench_ring_spsc_latency(c: &mut Criterion) {
    init_tsc();

    c.bench_function("ring_spsc_cross_thread", |b| {
        b.iter_custom(|iters| {
            let ring = Arc::new(RingBuffer::<u64, 4096>::new());
            let ring_consumer = Arc::clone(&ring);

            // Spawn consumer thread
            let consumer = thread::spawn(move || {
                for _ in 0..iters {
                    while ring_consumer.pop().is_none() {
                        std::hint::spin_loop();
                    }
                }
            });

            // Producer: measure time to push all items
            let start = std::time::Instant::now();
            for i in 0..iters {
                while ring.push(black_box(i)).is_err() {
                    std::hint::spin_loop();
                }
            }
            let elapsed = start.elapsed();

            consumer.join().unwrap();
            elapsed
        });
    });
}

criterion_group!(
    benches,
    bench_ring_push_pop,
    bench_ring_transaction,
    bench_ring_bulk_throughput,
    bench_ring_spsc_latency
);
criterion_main!(benches);

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use velox_engine::*;

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

criterion_group!(benches, bench_ring_push_pop, bench_ring_transaction);
criterion_main!(benches);

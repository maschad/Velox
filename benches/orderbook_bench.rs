use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use velox_engine::*;
use std::sync::Arc;
use std::thread;

fn bench_orderbook_update(c: &mut Criterion) {
    init_tsc();

    c.bench_function("orderbook_update_bid", |b| {
        let book = OrderBook::new();
        let mut price = 1000000;

        b.iter(|| {
            price += 1;
            book.update_bid(black_box(price), black_box(100), black_box(0))
                .unwrap();
        });
    });

    c.bench_function("orderbook_update_ask", |b| {
        let book = OrderBook::new();
        let mut price = 1000000;

        b.iter(|| {
            price += 1;
            book.update_ask(black_box(price), black_box(100), black_box(0))
                .unwrap();
        });
    });
}

fn bench_orderbook_contention(c: &mut Criterion) {
    init_tsc();

    let mut group = c.benchmark_group("orderbook_contention");

    // Benchmark with different number of concurrent updates to same level
    for contention_level in [1, 2, 4].iter() {
        group.bench_with_input(
            BenchmarkId::new("same_level", contention_level),
            contention_level,
            |b, &_level| {
                let book = OrderBook::new();
                let price = 1000000; // Same price = high contention

                b.iter(|| {
                    book.update_bid(black_box(price), black_box(10), black_box(0))
                        .unwrap();
                });
            },
        );
    }

    group.finish();
}

fn bench_orderbook_spread(c: &mut Criterion) {
    init_tsc();

    c.bench_function("orderbook_spread", |b| {
        let book = OrderBook::new();
        book.update_bid(1000000, 100, 0).unwrap();
        book.update_ask(1001000, 100, 0).unwrap();

        b.iter(|| {
            black_box(book.spread());
        });
    });
}

fn bench_orderbook_multithreaded(c: &mut Criterion) {
    init_tsc();

    let mut group = c.benchmark_group("orderbook_multithreaded");

    // 4-thread contention
    group.bench_function("4_threads", |b| {
        b.iter_custom(|iters| {
            let book = Arc::new(OrderBook::new());
            let mut handles = vec![];

            let start = std::time::Instant::now();

            for thread_id in 0..4 {
                let book = Arc::clone(&book);
                let handle = thread::spawn(move || {
                    for i in 0..iters {
                        let price = 1000000 + (thread_id * 1000) + (i % 100) as i64;
                        let _ = book.update_bid(price, 100, 0);
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                handle.join().unwrap();
            }

            start.elapsed()
        });
    });

    // 8-thread contention
    group.bench_function("8_threads", |b| {
        b.iter_custom(|iters| {
            let book = Arc::new(OrderBook::new());
            let mut handles = vec![];

            let start = std::time::Instant::now();

            for thread_id in 0..8 {
                let book = Arc::clone(&book);
                let handle = thread::spawn(move || {
                    for i in 0..iters {
                        let price = 1000000 + (thread_id * 1000) + (i % 100) as i64;
                        let _ = book.update_bid(price, 100, 0);
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                handle.join().unwrap();
            }

            start.elapsed()
        });
    });

    group.finish();
}

fn bench_orderbook_cas_pressure(c: &mut Criterion) {
    init_tsc();

    let mut group = c.benchmark_group("orderbook_cas_pressure");

    // Single level hammering with multiple threads
    group.bench_function("4_threads_single_level", |b| {
        b.iter_custom(|iters| {
            let book = Arc::new(OrderBook::new());
            let mut handles = vec![];

            // All threads target same price level to maximize CAS contention
            let price = 1000000i64;

            let start = std::time::Instant::now();

            for _ in 0..4 {
                let book = Arc::clone(&book);
                let handle = thread::spawn(move || {
                    for i in 0..iters {
                        let delta = ((i % 100) as i64) + 1;
                        let _ = book.update_bid(price, delta, 0);
                    }
                });
                handles.push(handle);
            }

            for handle in handles {
                handle.join().unwrap();
            }

            start.elapsed()
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_orderbook_update,
    bench_orderbook_contention,
    bench_orderbook_spread,
    bench_orderbook_multithreaded,
    bench_orderbook_cas_pressure
);
criterion_main!(benches);

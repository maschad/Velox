use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use velox_engine::*;

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

criterion_group!(
    benches,
    bench_orderbook_update,
    bench_orderbook_contention,
    bench_orderbook_spread
);
criterion_main!(benches);

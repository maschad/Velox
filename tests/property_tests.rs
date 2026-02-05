use proptest::prelude::*;
use velox_engine::*;

proptest! {
    /// Property: No transaction loss - input count = output count
    #[test]
    fn prop_no_transaction_loss(txns in prop::collection::vec(0u64..1000000, 1..100)) {
        init_tsc();
        let ring = RingBuffer::<u64, 1024>::new();

        let mut pushed = 0;
        for txn in &txns {
            if ring.push(*txn).is_ok() {
                pushed += 1;
            }
        }

        let mut popped = 0;
        while ring.pop().is_some() {
            popped += 1;
        }

        prop_assert_eq!(pushed, popped);
    }

    /// Property: FIFO order preserved
    #[test]
    fn prop_fifo_order(txns in prop::collection::vec(0u64..1000000, 1..100)) {
        init_tsc();
        let ring = RingBuffer::<u64, 1024>::new();

        let mut expected = Vec::new();
        for txn in &txns {
            if ring.push(*txn).is_ok() {
                expected.push(*txn);
            } else {
                break; // Buffer full
            }
        }

        let mut actual = Vec::new();
        while let Some(txn) = ring.pop() {
            actual.push(txn);
        }

        prop_assert_eq!(expected, actual);
    }

    /// Property: Order book best_bid <= best_ask (when both exist)
    #[test]
    fn prop_orderbook_spread_invariant(
        bid_price in 900000i64..1000000,
        ask_price in 1000000i64..1100000,
    ) {
        let book = OrderBook::new();

        book.update_bid(bid_price, 100, 0).unwrap();
        book.update_ask(ask_price, 100, 0).unwrap();

        let best_bid = book.best_bid();
        let best_ask = book.best_ask();

        if best_bid > 0 && best_ask < i64::MAX {
            prop_assert!(best_bid <= best_ask, "best_bid={} > best_ask={}", best_bid, best_ask);
        }
    }

    /// Property: Bundle size bounds (1 <= count <= BUNDLE_MAX)
    #[test]
    fn prop_bundle_size_bounds(count in 1usize..=BUNDLE_MAX) {
        init_tsc();
        let output_ring = RingBuffer::<Bundle, 1024>::new();
        let mut builder = BundleBuilder::new();

        for i in 0..count {
            let txn = Transaction::new_unchecked(i as u64, 1000000, 100, 0, 0);
            let _ = builder.add(txn, &output_ring);
        }

        // Force flush to get bundle
        builder.force_flush(&output_ring).ok();

        if let Some(bundle) = output_ring.pop() {
            prop_assert!(bundle.count >= 1 && bundle.count <= BUNDLE_MAX as u32);
        }
    }

    /// Property: Transaction serialization round-trip
    #[test]
    fn prop_transaction_serialization(
        id in 0u64..u64::MAX,
        price in i64::MIN..i64::MAX,
        size in 0u32..u32::MAX,
        side in 0u8..2,
    ) {
        let txn = Transaction::new_unchecked(id, price, size, side, 12345);
        let bytes = txn.to_bytes();
        let txn2 = Transaction::from_bytes(&bytes);

        prop_assert_eq!(txn.id, txn2.id);
        prop_assert_eq!(txn.price, txn2.price);
        prop_assert_eq!(txn.size, txn2.size);
        prop_assert_eq!(txn.side, txn2.side);
    }
}

#[cfg(test)]
mod stress_tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    #[ignore] // Run with: cargo test --release -- --ignored --nocapture
    fn stress_test_pipeline() {
        init_tsc();

        let ring = Arc::new(RingBuffer::<Transaction, 4096>::new());
        let book = Arc::new(OrderBook::new());

        let mut handles = vec![];

        // Spawn multiple producers
        for thread_id in 0..4 {
            let r = Arc::clone(&ring);
            let handle = thread::spawn(move || {
                for i in 0..100_000 {
                    let txn = Transaction::new_unchecked(
                        (thread_id * 100_000 + i) as u64,
                        1000000,
                        100,
                        0,
                        0,
                    );

                    while r.push(txn).is_err() {
                        std::hint::spin_loop();
                    }
                }
            });
            handles.push(handle);
        }

        // Spawn multiple consumers
        for _ in 0..4 {
            let r = Arc::clone(&ring);
            let b = Arc::clone(&book);
            let handle = thread::spawn(move || {
                let mut processed = 0;
                while processed < 100_000 {
                    if let Some(txn) = r.pop() {
                        let _ = b.update_bid(txn.price, txn.size as i64, 0);
                        processed += 1;
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        println!("Stress test completed successfully");
    }

    #[test]
    #[ignore]
    fn stress_test_long_run() {
        init_tsc();

        let ring = Arc::new(RingBuffer::<Transaction, 4096>::new());
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let s1 = Arc::clone(&stop);
        let r1 = Arc::clone(&ring);
        let producer = thread::spawn(move || {
            let mut id = 0;
            while !s1.load(std::sync::atomic::Ordering::Relaxed) {
                let txn = Transaction::new_unchecked(id, 1000000, 100, 0, 0);
                if r1.push(txn).is_ok() {
                    id += 1;
                }
            }
            id
        });

        let s2 = Arc::clone(&stop);
        let r2 = Arc::clone(&ring);
        let consumer = thread::spawn(move || {
            let mut count = 0;
            while !s2.load(std::sync::atomic::Ordering::Relaxed) {
                if r2.pop().is_some() {
                    count += 1;
                }
            }
            // Drain remaining
            while r2.pop().is_some() {
                count += 1;
            }
            count
        });

        // Run for 10 seconds
        thread::sleep(Duration::from_secs(10));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);

        let produced = producer.join().unwrap();
        let consumed = consumer.join().unwrap();

        println!("Long run: produced={} consumed={}", produced, consumed);
        assert_eq!(produced, consumed, "Transaction loss detected!");
    }
}

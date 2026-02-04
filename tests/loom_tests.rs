#[cfg(loom)]
mod loom_tests {
    use loom::sync::atomic::{AtomicU64, Ordering};
    use loom::sync::Arc;
    use loom::thread;

    /// Test SPSC ring buffer under loom model checker
    /// This is a simplified model since loom has limitations
    #[test]
    fn test_spsc_ring_basic() {
        loom::model(|| {
            let head = Arc::new(AtomicU64::new(0));
            let tail = Arc::new(AtomicU64::new(0));

            let h1 = Arc::clone(&head);
            let t1 = Arc::clone(&tail);

            // Producer
            let producer = thread::spawn(move || {
                let current_head = h1.load(Ordering::Relaxed);
                let current_tail = t1.load(Ordering::Acquire);

                // Check if buffer is full (simplified: capacity = 4)
                if current_head.wrapping_sub(current_tail) < 4 {
                    h1.store(current_head + 1, Ordering::Release);
                }
            });

            let h2 = Arc::clone(&head);
            let t2 = Arc::clone(&tail);

            // Consumer
            let consumer = thread::spawn(move || {
                let current_tail = t2.load(Ordering::Relaxed);
                let current_head = h2.load(Ordering::Acquire);

                // Check if buffer is empty
                if current_tail != current_head {
                    t2.store(current_tail + 1, Ordering::Release);
                }
            });

            producer.join().unwrap();
            consumer.join().unwrap();

            // Verify invariants
            let final_head = head.load(Ordering::Relaxed);
            let final_tail = tail.load(Ordering::Relaxed);
            assert!(final_head >= final_tail);
        });
    }

    /// Test order book CAS updates
    #[test]
    fn test_orderbook_cas() {
        loom::model(|| {
            let level = Arc::new(AtomicU64::new(0));

            let l1 = Arc::clone(&level);
            let t1 = thread::spawn(move || {
                let current = l1.load(Ordering::Acquire);
                let _ = l1.compare_exchange_weak(
                    current,
                    current + 100,
                    Ordering::Release,
                    Ordering::Relaxed,
                );
            });

            let l2 = Arc::clone(&level);
            let t2 = thread::spawn(move || {
                let current = l2.load(Ordering::Acquire);
                let _ = l2.compare_exchange_weak(
                    current,
                    current + 200,
                    Ordering::Release,
                    Ordering::Relaxed,
                );
            });

            t1.join().unwrap();
            t2.join().unwrap();

            let final_val = level.load(Ordering::Relaxed);
            // One of the updates should succeed
            assert!(final_val == 100 || final_val == 200 || final_val == 300);
        });
    }
}

// Regular tests (non-loom)
#[cfg(not(loom))]
mod regular_tests {
    use velox_engine::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_concurrent_ring_push_pop() {
        init_tsc();
        let ring = Arc::new(RingBuffer::<u64, 4096>::new());

        let r1 = Arc::clone(&ring);
        let producer = thread::spawn(move || {
            for i in 0..1000 {
                while r1.push(i).is_err() {
                    std::hint::spin_loop();
                }
            }
        });

        let r2 = Arc::clone(&ring);
        let consumer = thread::spawn(move || {
            let mut count = 0;
            let mut last = None;
            while count < 1000 {
                if let Some(val) = r2.pop() {
                    if let Some(prev) = last {
                        assert_eq!(val, prev + 1, "FIFO order violated");
                    }
                    last = Some(val);
                    count += 1;
                }
            }
        });

        producer.join().unwrap();
        consumer.join().unwrap();
    }

    #[test]
    fn test_concurrent_orderbook_updates() {
        let book = Arc::new(OrderBook::new());
        let mut handles = vec![];

        for i in 0..4 {
            let b = Arc::clone(&book);
            let handle = thread::spawn(move || {
                for j in 0..100 {
                    let price = 1000 + (i * 100) + j;
                    let _ = b.update_bid(price, 10, 0);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Verify best bid is updated
        assert!(book.best_bid() > 0);
    }
}

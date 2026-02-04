# Velox — CLAUDE.md

## Project Context
Lock-free transaction pipeline simulating HFT infrastructure (Click.Trade style).
Interview prep. Every design decision should be defensible in a technical interview.

## Rules
- No Mutex, no Arc<Mutex<>>, no RwLock on the hot path. Ever.
- No heap allocation in Transaction, Bundle, or ring buffer hot paths.
- All structs on the hot path are repr(C).
- Benchmark BEFORE optimizing. Always.
- When adding OTel instrumentation, add it at STAGE BOUNDARIES only, never inside ring buffer push/pop.
- Comments on atomic orderings are MANDATORY. Explain WHY you chose that ordering.

## File Layout
- src/ring.rs       — SPSC ring buffer (lock-free, the core primitive)
- src/types.rs      — Transaction, Bundle (repr(C), zero-alloc)
- src/orderbook.rs  — Lock-free order book
- src/bundle.rs     — Bundle builder (accumulate + flush)
- src/telemetry.rs  — OTel setup (metrics + traces)
- src/main.rs       — Pipeline wiring, thread spawn, shutdown
- benches/bench.rs  — Criterion benchmarks

## Interview Prep Mode
After every implementation, ask: "How would I explain this design choice in a 5-minute interview answer?"

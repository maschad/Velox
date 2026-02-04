# 🚢 Ship Checklist

## Pre-Deployment Validation

Run these commands to verify everything is production-ready:

### 1. Build and Test

```bash
# Clean build
cargo clean
cargo build --release

# Run all tests
cargo test --lib --release

# Expected output:
# test result: ok. 27 passed; 0 failed; 1 ignored
```

### 2. Verify Pipeline Operation

```bash
# Run pipeline for 10 seconds
cargo run --release

# Check for:
✅ "TSC initialized and calibrated" (before any other output)
✅ Throughput ~99k txn/sec
✅ "Shutting down gracefully..."
✅ "Draining buffers..."
✅ "Drained: X transactions, Y bundles"
✅ "dropped=0" in statistics
```

### 3. Performance Validation

```bash
# Run benchmarks
cargo bench

# Expected:
✅ ring_buffer: <10ns
✅ orderbook: <10ns
✅ e2e: <50ns
```

### 4. CPU Usage Validation

```bash
# Terminal 1: Run pipeline
cargo run --release

# Terminal 2: Monitor CPU
top -pid $(pgrep velox-engine) -stats cpu

# Expected:
✅ CPU: ~100% when processing
✅ CPU: <10% when idle (after 10 seconds)
```

### 5. Memory Safety Check

```bash
# Run all tests including ignored
cargo test --lib -- --include-ignored

# Run property tests
cargo test --test property_tests

# Run concurrent tests
cargo test --test loom_tests

# All should pass ✅
```

## Deployment Checklist

### Critical (Must verify before deploy)

- [ ] All tests passing (27/27)
- [ ] Pipeline runs without panics
- [ ] Shutdown drains buffers (zero data loss)
- [ ] CPU usage acceptable (<10% idle)
- [ ] No compiler warnings
- [ ] Documentation reviewed

### Recommended (Before production)

- [ ] Run soak test (24 hours)
- [ ] Add logging/metrics
- [ ] Add configuration file
- [ ] Set up monitoring/alerting
- [ ] Create runbook for operations

### Deployment Steps

1. **Build release binary**:
   ```bash
   cargo build --release
   strip target/release/velox-engine  # Remove debug symbols
   ```

2. **Copy to deployment location**:
   ```bash
   scp target/release/velox-engine user@server:/opt/velox/
   ```

3. **Run with supervision** (systemd, supervisor, etc.):
   ```bash
   ./velox-engine 2>&1 | tee velox.log
   ```

4. **Monitor for issues**:
   ```bash
   # Check process is running
   ps aux | grep velox-engine

   # Monitor output
   tail -f velox.log

   # Check CPU/memory
   top -pid $(pgrep velox-engine)
   ```

## Known Limitations

⚠️ **Read ORDERBOOK_LIMITATIONS.md before deploying!**

Key points:
- Order book uses price bucketing (16 ticks per level)
- Multiple prices map to same bucket (aggregated)
- Best bid/ask are approximate (±15 ticks accuracy)
- NOT suitable for order matching or precise P&L

Good for:
- ✅ High-frequency transaction analytics
- ✅ MEV detection pipelines
- ✅ Volume tracking
- ✅ Price momentum signals

## Rollback Plan

If issues occur in production:

```bash
# Stop the process
kill -TERM $(pgrep velox-engine)

# Process will:
# 1. Detect shutdown signal
# 2. Drain buffers (zero data loss)
# 3. Exit cleanly

# Check final statistics in output
tail -50 velox.log
```

## Support

- **Documentation**: See README.md, ORDERBOOK_LIMITATIONS.md
- **Architecture**: See CLAUDE.md
- **Issues**: See FIXES_APPLIED.md, PRODUCTION_READY.md
- **Skills**: See skills/ directory

## Success Metrics

After deployment, validate:

- [ ] No panics/crashes in first 24 hours
- [ ] Throughput meets target (>95k txn/sec)
- [ ] Drop rate <1%
- [ ] CPU usage acceptable
- [ ] Memory stable (no leaks)
- [ ] Clean shutdown on SIGTERM

If all ✅, you're good to continue running!

---

**Last Updated**: 2026-02-03
**Status**: 🟢 READY TO SHIP

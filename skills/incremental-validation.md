# Incremental Validation

Test and validate after each component/phase instead of waiting until the end. Catches bugs early when they're cheap to fix, prevents cascade failures.

## When to use

- Building complex systems with multiple components
- Performance-critical applications where late optimization is expensive
- Projects with >1 week duration
- When components depend on each other (Phase N uses Phase N-1)
- Collaborative projects where others build on your work

## When NOT to use

- Throwaway prototypes
- Simple scripts with no dependencies
- When requirements will change drastically
- Pure research/exploration (validation would slow discovery)

## Instructions

### Step 1: Define Validation Levels

For each component/phase, specify what "done" means:

```markdown
## Phase N: [Component Name]

### Validation Checklist:

**Correctness:**
- [ ] Unit tests pass ([X] tests)
- [ ] Edge cases covered (empty input, max size, etc.)
- [ ] Error handling verified

**Performance:**
- [ ] Latency target met: <[X] ns/μs/ms
- [ ] Throughput target met: >[Y] ops/sec
- [ ] Memory usage: <[Z] KB/MB

**Integration:**
- [ ] Integrates with Phase [N-1] output
- [ ] Provides correct input for Phase [N+1]
- [ ] No breaking API changes from plan

**Code Quality:**
- [ ] No compiler warnings
- [ ] Passes linter/formatter
- [ ] Documentation updated
```

### Step 2: Validate Immediately After Implementation

**Strict rule:** Don't start Phase N+1 until Phase N validation passes.

**Workflow:**
```bash
# Implement Phase 1
vim src/module.rs

# Validate immediately (don't move on!)
cargo test --lib module::tests  # Unit tests
cargo bench --bench module_bench  # Performance
cargo clippy  # Linting

# Only if ALL pass:
git commit -m "Phase 1: Module complete and validated"

# Now start Phase 2
```

### Step 3: Create Fast Feedback Loop

Optimize for speed of validation:

**Fast validation (seconds):**
```bash
# Unit tests only (no integration)
cargo test --lib module::tests

# Specific test
cargo test test_name

# Compile check
cargo check
```

**Medium validation (minutes):**
```bash
# All tests
cargo test

# Benchmarks (quick)
cargo bench --bench module -- --quick
```

**Slow validation (hours - run overnight/CI):**
```bash
# Stress tests
cargo test --release -- --ignored --nocapture

# Full benchmark suite
cargo bench

# Integration tests
cargo test --test '*'
```

### Step 4: Fix Issues Before Proceeding

If validation fails:

1. **Stop** - Don't continue to next phase
2. **Diagnose** - Understand root cause
3. **Fix** - Correct the issue
4. **Re-validate** - Run validation again
5. **Document** - Note what was fixed and why

**Anti-pattern:**
```
Phase 1 fails validation → "I'll fix it later"
Phase 2 builds on broken Phase 1
Phase 3 builds on broken Phase 2
[Weeks later: Massive refactoring needed]
```

**Correct pattern:**
```
Phase 1 fails validation → Fix immediately
Phase 1 passes → Proceed to Phase 2
Phase 2 uses validated Phase 1
Phase 3 uses validated Phase 2
[Result: Solid foundation, no surprises]
```

### Step 5: Document Validation Results

After each phase, record:

```markdown
## Phase N Validation Results

**Tests:** ✅ 15/15 passing
**Performance:** ✅ 32ns mean (target: <50ns)
**Issues found:** 1 (Transaction size alignment)
**Issues fixed:** Array padding added, tests now pass
**Time to validate:** 5 minutes
**Time to fix issues:** 15 minutes

**Lessons learned:**
- Rust's default struct layout != expected
- Added compile-time assertion to catch early
```

## Examples

### Example 1: Lock-Free Queue (Velox Engine)

**Phase 1: Core Types**
```rust
// Implement Transaction struct
#[repr(C)]
struct Transaction { ... }

// Validate IMMEDIATELY
#[test]
fn test_transaction_size() {
    assert_eq!(size_of::<Transaction>(), 32);  // FAILED!
}
```

**Result:** Found size was 33 bytes due to padding
**Fix:** Reordered fields, added explicit padding
**Time:** 15 minutes to fix vs. hours later if discovered in Phase 5

**Phase 2: Ring Buffer**
```rust
// Implement push/pop
impl RingBuffer { ... }

// Validate IMMEDIATELY
#[test]
fn test_fifo_order() { ... }  // PASSED

cargo bench --bench ring  // 4ns - target was <50ns ✅
```

**Result:** Performance excellent, moved to Phase 3

### Example 2: REST API

**Phase 1: Database Schema**
```sql
CREATE TABLE users (...);
```

**Validation:**
```bash
# Run migrations
diesel migration run

# Seed test data
psql < test_data.sql

# Verify constraints
SELECT * FROM users WHERE invalid_condition;  # Should be empty
```

**Result:** Foreign key constraint incorrect
**Fix:** Update migration, re-run
**Time:** 5 minutes vs. discovering in production

**Phase 2: Repository Layer**
```rust
impl UserRepository {
    fn create(...) { ... }
    fn find_by_id(...) { ... }
}
```

**Validation:**
```bash
cargo test repository::user::tests
```

**Result:** find_by_id returns wrong data
**Fix:** SQL query had wrong JOIN
**Time:** 10 minutes vs. debugging customer issues

## Best Practices

### ✅ Do

- **Test immediately** - Within seconds of writing code
- **Fix before proceeding** - No moving on with broken code
- **Automate validation** - Scripts, make targets, CI
- **Document issues** - What broke, why, how fixed
- **Celebrate green tests** - Positive reinforcement
- **Use watch mode** - `cargo watch -x test` for instant feedback

### ❌ Don't

- **Don't defer testing** - "I'll test it all at the end"
- **Don't skip on time pressure** - Testing saves time overall
- **Don't ignore flaky tests** - Fix or remove them
- **Don't validate too broadly** - Test specific component, not everything
- **Don't over-validate** - Match validation depth to component risk

## Validation Strategy by Component Type

### Core Data Structures
```yaml
Validation:
  - Size/alignment assertions (compile-time)
  - Serialization round-trip
  - Edge cases (empty, max size)
  - Memory leaks (valgrind/asan)
Priority: HIGH (everything builds on this)
```

### Lock-Free Algorithms
```yaml
Validation:
  - Unit tests (basic correctness)
  - Loom tests (concurrency model checking)
  - Property tests (invariants)
  - Stress tests (1M+ operations)
Priority: CRITICAL (subtle bugs hard to find later)
```

### Business Logic
```yaml
Validation:
  - Unit tests (all branches)
  - Integration tests (with dependencies)
  - Error cases
Priority: MEDIUM (bugs obvious in manual testing)
```

### Performance-Critical Code
```yaml
Validation:
  - Benchmarks (P50, P99 latency)
  - Profiling (flamegraph)
  - Cache miss rate (perf stat)
Priority: HIGH (late optimization expensive)
```

## Common Pitfalls

### Pitfall 1: "I'll test it all at the end"
**Problem:**
- Build 10 components
- Test at end
- Find bug in Component 1
- All 9 other components built on broken foundation
- **Result:** Days/weeks of refactoring

**Solution:** Test Component 1 immediately, validate before building Component 2

### Pitfall 2: Validation too slow
**Problem:**
- Full test suite takes 10 minutes
- Run after every small change
- **Result:** 80% of time waiting for tests

**Solution:**
```bash
# Fast: Test only what you changed
cargo test --lib module::specific_test  # <1 second

# Medium: Full unit tests
cargo test --lib  # ~10 seconds

# Slow: Everything
cargo test  # 10 minutes - run before commit only
```

### Pitfall 3: Ignoring performance until end
**Problem:**
- Build entire system
- Test functionality - all passes ✅
- Measure performance - 10x too slow ❌
- **Result:** No idea where bottleneck is, massive profiling effort

**Solution:** Benchmark each component as built:
```markdown
Phase 2: Ring Buffer
- Target: <50ns per push/pop
- Measured: 4ns ✅
- Proceed to Phase 3

Phase 3: Order Book
- Target: <200ns per update
- Measured: 3ns ✅
- Proceed to Phase 4
```

### Pitfall 4: Validation scope creep
**Problem:**
- Testing Phase 2
- Also re-test Phase 1
- Also test integration with future Phase 4
- **Result:** Slow feedback loop, unclear what's being validated

**Solution:** Validate only what changed:
```bash
# YES: Test Phase 2 component
cargo test --lib orderbook::tests

# NO: Test entire system
cargo test  # Too broad
```

## Measuring Success

### Indicators it's working:
- ✅ Bugs caught in same phase they're introduced
- ✅ Rarely need to revisit earlier phases
- ✅ Tests stay green 90%+ of time
- ✅ No "surprise" bugs at end
- ✅ Integration smooth (components work together first try)

### Indicators it's failing:
- ❌ Bugs discovered 3-5 phases after introduction
- ❌ Constant refactoring of early phases
- ❌ "Works on my machine" but fails in integration
- ❌ Performance surprises at end
- ❌ More time fixing than building new features

## Integration with CI/CD

### Pre-commit Hook
```bash
#!/bin/bash
# .git/hooks/pre-commit

echo "Running validation..."

# Fast checks only
cargo fmt --check || exit 1
cargo clippy -- -D warnings || exit 1
cargo test --lib || exit 1

echo "✅ Validation passed"
```

### PR Validation
```yaml
# .github/workflows/pr.yml
on: pull_request

jobs:
  validate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - name: Run tests
        run: cargo test
      - name: Run benchmarks
        run: cargo bench -- --quick
      - name: Check performance regression
        run: ./scripts/check_perf_regression.sh
```

### Per-Phase Branches
```bash
# Start phase
git checkout -b phase-2-ring-buffer

# Implement + validate
vim src/ring.rs
cargo test --lib ring::tests
cargo bench --bench ring

# Only merge if validation passes
git checkout main
git merge phase-2-ring-buffer  # CI validates again
```

## Advanced: Validation Matrix

| Component | Unit | Integration | Property | Stress | Bench | Priority |
|-----------|------|-------------|----------|--------|-------|----------|
| Core Types | ✅ | - | ✅ | - | - | HIGH |
| Ring Buffer | ✅ | - | ✅ | ✅ | ✅ | CRITICAL |
| Order Book | ✅ | - | ✅ | ✅ | ✅ | CRITICAL |
| Bundle Builder | ✅ | ✅ | - | - | ✅ | MEDIUM |
| Pipeline | - | ✅ | - | ✅ | ✅ | HIGH |

**Legend:**
- Unit: `cargo test --lib component::tests`
- Integration: `cargo test --test integration_tests`
- Property: `cargo test --test property_tests`
- Stress: `cargo test --release -- --ignored`
- Bench: `cargo bench --bench component_bench`

## References

- Real example: Velox Engine (7 phases, validated incrementally)
- Transaction size bug caught in Phase 1 (15 min to fix)
- Memory ordering validated in Phase 2 (prevented race conditions)
- Performance validated per phase (all targets exceeded)
- Zero architectural refactoring needed (solid foundation)

## Related Skills

- `plan-first-development` - Define what to validate per phase
- `parallel-subagent-orchestration` - Run validations in parallel
- `multi-level-testing` - Different test types for validation

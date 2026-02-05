# Generalized Claude Skills for Rust Systems Programming

This document contains reusable prompts and patterns extracted from the Velox Engine development. Use these as starting points for similar projects.

---

## Skill 1: Lock-Free Data Structure Implementation

**When to use**: Building concurrent data structures without locks (queues, stacks, hash tables)

**Prompt Template**:
```
Implement a lock-free [DATA_STRUCTURE] in Rust with the following requirements:

Architecture:
- Target platform: [ARM64/x86_64/both]
- Concurrency model: [SPSC/MPSC/MPMC]
- Memory ordering: Release/Acquire (for ARM compatibility)

Requirements:
- Zero heap allocation in hot path
- Cache-line padding (64 bytes) on shared atomics
- Power-of-two size for fast modulo operations
- Use UnsafeCell<MaybeUninit<T>> for uninitialized slots
- Bounded operations (no infinite loops)

Implementation checklist:
- [ ] Define structure with cache-padded atomics
- [ ] Implement push/pop with proper memory ordering
- [ ] Add backpressure handling (return error on full)
- [ ] Write safety documentation for unsafe code
- [ ] Add compile-time assertions for alignment
- [ ] Create unit tests + Loom concurrency tests
```

**Key Patterns to Include**:
```rust
// Cache-line padding
#[repr(C, align(64))]
struct CachePadded<T> {
    value: T,
}

// Release/Acquire synchronization
producer_atomic.store(value, Ordering::Release);
let value = consumer_atomic.load(Ordering::Acquire);

// Power-of-two indexing
const N: usize = 4096; // Must be power of 2
let index = (position as usize) & (N - 1); // Fast modulo
```

---

## Skill 2: High-Performance Memory Layout Design

**When to use**: Designing cache-friendly structs for hot paths

**Prompt Template**:
```
Design a memory layout for [STRUCT_NAME] with these requirements:

Performance constraints:
- Must fit in [L1/L2/L3] cache
- Zero heap allocation
- Cache-line alignment for shared fields
- Fixed size for array storage

Implementation:
1. Use #[repr(C)] for predictable layout
2. Order fields by size (largest first) to minimize padding
3. Add explicit padding bytes for alignment
4. Use static_assertions to verify size at compile time
5. Implement Copy trait if appropriate

Example struct with validation:
```rust
#[repr(C)]
#[derive(Copy, Clone)]
pub struct MyStruct {
    field1: u64,
    field2: u64,
    field3: u32,
    _padding: u32,  // Explicit padding
}

const_assert_eq!(core::mem::size_of::<MyStruct>(), 24);
const_assert_eq!(core::mem::align_of::<MyStruct>(), 8);
```

**Size Budget Checklist**:
- [ ] Calculate total size (sum of fields + padding)
- [ ] Verify alignment requirements
- [ ] Check if size is power of 2 (cache-friendly)
- [ ] Document memory layout rationale
```

---

## Skill 3: Cross-Platform TSC/Timer Implementation

**When to use**: Need microsecond/nanosecond precision timing

**Prompt Template**:
```
Implement cross-platform timing utilities with TSC/high-resolution counters:

Requirements:
- Support ARM64 (CNTVCT_EL0) and x86_64 (RDTSC)
- Calibrate at startup to convert ticks to nanoseconds
- Provide conversion functions (tsc_to_ns, ns_to_tsc)
- Implement spin_sleep for precise delays

Implementation steps:
1. Create platform-specific rdtsc() functions with inline assembly
2. Implement calibration by sleeping known duration
3. Store calibration factor in static OnceCell
4. Add conversion utilities
5. Test calibration accuracy (within 10% tolerance)

Example usage:
```rust
// At startup
init_tsc();

// In hot path
let start = rdtsc();
do_work();
let elapsed_ns = tsc_to_ns(rdtsc() - start);
```

Platform notes:
- ARM64: CNTVCT_EL0 is always available at EL0
- x86_64: RDTSC may be serializing, use RDTSCP if available
- Both: Assume invariant TSC (stable on modern CPUs)
```

---

## Skill 4: Bounded Retry with Exponential Backoff

**When to use**: Contended atomic operations (CAS loops, spinlocks)

**Prompt Template**:
```
Implement a bounded CAS retry loop with exponential backoff:

Parameters:
- Max retries: [NUMBER] (e.g., 100)
- Initial backoff: 1 spin loop iteration
- Max backoff: 64 spin loop iterations
- Return error on timeout

Pattern:
```rust
const MAX_RETRIES: usize = 100;

pub fn update(&self, value: T) -> Result<(), Timeout> {
    let mut backoff = 1;

    for _ in 0..MAX_RETRIES {
        let current = self.atomic.load(Ordering::Acquire);
        let new_value = compute(current, value);

        match self.atomic.compare_exchange_weak(
            current,
            new_value,
            Ordering::Release,  // Success
            Ordering::Relaxed,  // Failure (will retry)
        ) {
            Ok(_) => return Ok(()),
            Err(_) => {
                // Exponential backoff
                for _ in 0..backoff {
                    core::hint::spin_loop();
                }
                backoff = (backoff * 2).min(64);
            }
        }
    }

    Err(Timeout)
}
```

Why this works:
- compare_exchange_weak: Faster on ARM, allows spurious failure
- Exponential backoff: Reduces contention as threads back off
- Bounded retry: Guarantees progress (no infinite loops)
- spin_loop hint: Tells CPU this is a spin loop (power saving)
```

---

## Skill 5: Zero-Copy Serialization

**When to use**: Need to serialize structs without allocation (network, IPC, shared memory)

**Prompt Template**:
```
Implement zero-copy serialization for [STRUCT_NAME]:

Requirements:
- No heap allocation
- #[repr(C)] for stable layout
- Fixed size (known at compile time)
- Bidirectional (to_bytes/from_bytes)

Implementation:
```rust
#[repr(C)]
pub struct MyStruct {
    // ... fields ...
}

impl MyStruct {
    pub fn to_bytes(&self) -> [u8; SIZE] {
        unsafe {
            let mut bytes = [0u8; SIZE];
            ptr::copy_nonoverlapping(
                self as *const Self as *const u8,
                bytes.as_mut_ptr(),
                SIZE,
            );
            bytes
        }
    }

    pub fn from_bytes(bytes: &[u8; SIZE]) -> Self {
        unsafe {
            let mut value = MaybeUninit::<Self>::uninit();
            ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                value.as_mut_ptr() as *mut u8,
                SIZE,
            );
            value.assume_init()
        }
    }
}
```

Safety requirements:
- [ ] Struct is #[repr(C)] (stable layout)
- [ ] Struct is Copy (no Drop, no heap pointers)
- [ ] All fields are valid for any bit pattern (no invalid states)
- [ ] Size matches expectation (use const_assert!)
```

---

## Skill 6: Thread Affinity and Pipeline Orchestration

**When to use**: Multi-threaded pipeline with dedicated cores

**Prompt Template**:
```
Implement a multi-threaded pipeline with CPU affinity:

Pipeline topology:
```
[Stage 1] → [RingBuffer] → [Stage 2] → [RingBuffer] → [Stage 3]
(Core 0)                    (Core 1)                    (Core 2)
```

Requirements:
- Pin each thread to dedicated core
- Use lock-free ring buffers between stages
- Atomic statistics for monitoring
- Graceful shutdown with shared flag

Implementation pattern:
```rust
use core_affinity::{set_for_current, CoreId};
use std::sync::Arc;

fn main() {
    // Create ring buffers
    let ring1 = Arc::new(RingBuffer::new());
    let ring2 = Arc::new(RingBuffer::new());
    let shutdown = Arc::new(AtomicBool::new(false));

    // Spawn stage threads
    let stage1 = {
        let ring = Arc::clone(&ring1);
        let shutdown = Arc::clone(&shutdown);
        thread::spawn(move || {
            if let Some(core) = (CoreId { id: 0 }).into() {
                set_for_current(core);
            }
            worker1(&ring, &shutdown);
        })
    };

    // Wait for completion
    thread::sleep(Duration::from_secs(RUN_TIME));
    shutdown.store(true, Ordering::Relaxed);
    stage1.join().unwrap();
}

fn worker1(output: &RingBuffer<T>, shutdown: &AtomicBool) {
    while !shutdown.load(Ordering::Relaxed) {
        // Produce work
        let item = produce();

        // Push to next stage (drop on full)
        let _ = output.push(item);
    }
}
```

Monitoring pattern:
- Use AtomicU64 for counters (relaxed ordering is fine)
- Separate monitor thread prints stats every second
- Avoid synchronization on hot path
```

---

## Skill 7: Comprehensive Testing Strategy

**When to use**: Testing concurrent/lock-free code

**Prompt Template**:
```
Create a comprehensive test suite for [COMPONENT]:

Test levels:
1. Unit tests (cargo test)
2. Property tests (proptest)
3. Concurrency tests (loom)
4. Stress tests (long-running)
5. Benchmarks (criterion)

Test checklist:
```rust
// 1. Unit tests
#[test]
fn test_basic_operation() {
    let x = create();
    assert!(x.operation());
}

// 2. Property tests
proptest! {
    #[test]
    fn prop_invariant(input in 0u64..1000) {
        let x = create();
        // Check invariant holds for all inputs
        prop_assert!(x.invariant());
    }
}

// 3. Loom concurrency tests
#[cfg(loom)]
#[test]
fn test_concurrent() {
    loom::model(|| {
        let x = Arc::new(create());

        let t1 = thread::spawn({
            let x = Arc::clone(&x);
            move || x.operation()
        });

        let t2 = thread::spawn({
            let x = Arc::clone(&x);
            move || x.operation()
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

// 4. Stress tests
#[test]
#[ignore]
fn stress_test_long_run() {
    // Run for 1 hour at high load
    // Check for panics, deadlocks, memory leaks
}

// 5. Benchmarks
fn bench_operation(c: &mut Criterion) {
    c.bench_function("operation", |b| {
        b.iter(|| {
            black_box(operation());
        });
    });
}
```

Invariants to test:
- [ ] No data loss (input count = output count)
- [ ] Order preservation (FIFO/LIFO)
- [ ] Bounds checking (min <= value <= max)
- [ ] Resource cleanup (no leaks)
- [ ] Progress guarantee (no deadlocks)
```

---

## Skill 8: Memory-Order Reasoning

**When to use**: Debugging or designing lock-free algorithms

**Prompt Template**:
```
Analyze memory ordering requirements for [OPERATION]:

Memory model considerations:
- x86_64: Strong memory model (loads/stores are seq-cst by default)
- ARM64: Weak memory model (requires explicit ordering)

Ordering hierarchy (weakest to strongest):
1. Relaxed: No synchronization, only atomicity
2. Acquire: Reads after this load can't move before it
3. Release: Writes before this store can't move after it
4. AcqRel: Combination of Acquire and Release
5. SeqCst: Total ordering across all threads

Common patterns:

Producer-Consumer (SPSC):
```rust
// Producer
slot.write(value);              // Regular write
head.store(new_head, Release);  // Publish to consumer

// Consumer
let h = head.load(Acquire);     // Synchronize with producer
let value = slot.read();        // Regular read
```

Spin Lock:
```rust
// Acquire lock
while lock.compare_exchange_weak(0, 1, Acquire, Relaxed).is_err() {
    spin_loop();
}

// Release lock
lock.store(0, Release);
```

Statistics Counter:
```rust
// Increment (no synchronization needed)
counter.fetch_add(1, Relaxed);

// Read latest value
let value = counter.load(Relaxed);
```

Decision tree:
1. Do threads need to synchronize data? → Yes: Release/Acquire or stronger
2. Is this just a counter? → No sync needed: Relaxed
3. Modifying shared state? → Yes: consider SeqCst or Mutex
4. Reading shared state? → Use Acquire to pair with Release stores
```

---

## Skill 9: Performance Validation Checklist

**When to use**: Verifying performance requirements

**Prompt Template**:
```
Create a performance validation plan for [SYSTEM]:

Validation categories:

1. Memory characteristics:
   - [ ] Working set size (should fit in L2/L3 cache)
   - [ ] Alignment (64-byte for cache lines)
   - [ ] Zero heap allocations on hot path (verify with heaptrack)
   - [ ] Cache miss rate <1% (measure with perf stat)

2. Latency:
   - [ ] P50 latency: [TARGET]
   - [ ] P99 latency: [TARGET]
   - [ ] P99.9 latency: [TARGET]
   - [ ] Measure end-to-end and per-stage

3. Throughput:
   - [ ] Sustained: [TARGET] ops/sec
   - [ ] Peak: [TARGET] ops/sec
   - [ ] Under load for [DURATION]

4. Concurrency:
   - [ ] No data races (ThreadSanitizer clean)
   - [ ] No deadlocks (stress test for 1 hour)
   - [ ] Bounded wait times (no infinite loops)
   - [ ] Scalability with cores

5. Resource usage:
   - [ ] CPU: [TARGET]% per core
   - [ ] Memory: [TARGET] KB/MB
   - [ ] Drop rate: <[TARGET]%

Measurement tools:
```bash
# Cache analysis
perf stat -e cache-references,cache-misses ./binary

# Heap tracking
heaptrack ./binary

# Thread safety
RUSTFLAGS="-Z sanitizer=thread" cargo build

# Latency distribution
cargo bench --bench e2e_bench
```

Acceptance criteria:
- All P99 latencies below target
- Zero heap allocations measured
- ThreadSanitizer passes
- Stress test runs without panic
```

---

## Skill 10: Structured Development Log

**When to use**: Documenting complex system development

**Prompt Template**:
```
Create a development log (CLAUDE.md) with:

Structure:
1. Project Overview
2. Development Timeline (per phase)
3. Key Patterns Developed
4. Challenges & Solutions
5. Performance Results
6. Lessons Learned
7. Reusable Components
8. Future Improvements
9. References

For each phase:
- Objective: What are we building?
- Approach: How did we build it?
- Key Decisions: Why did we choose this design?
- Challenges & Solutions: What went wrong and how did we fix it?

Key Patterns section should include:
- Code snippets
- When to use each pattern
- Rationale for the design

Example phase entry:
```markdown
### SPSC Ring Buffer (Phase 2)
**Objective**: Build lock-free single-producer, single-consumer queue

**Approach**:
1. Cache-line padded atomics (64 bytes)
2. Release/Acquire memory ordering
3. Power-of-two sizes for fast modulo

**Key Decisions**:
- Backpressure on full: return Err instead of blocking
- Producer owns head, consumer owns tail

**Challenges & Solutions**:
- **Challenge**: Understanding ARM memory ordering
- **Solution**: Used Release/Acquire pairing for synchronization
```

This creates a reference document for future projects.
```

---

## Meta-Skill: Plan-Driven Development

**When to use**: Starting any complex systems project

**Process**:
1. **Create detailed plan** (like the one provided for Velox Engine)
   - Break into phases
   - List all files to create
   - Specify data structures and algorithms
   - Define success criteria

2. **Implement phase by phase**
   - Complete each phase fully before moving to next
   - Write tests immediately after implementation
   - Update CLAUDE.md after each phase

3. **Validate continuously**
   - Run tests after each component
   - Check performance early and often
   - Fix issues immediately (don't accumulate debt)

4. **Document as you go**
   - Update CLAUDE.md with challenges/solutions
   - Extract reusable patterns
   - Note performance measurements

5. **Create reusable artifacts**
   - Generalized skills document
   - Benchmark harnesses
   - Test utilities

**Benefits**:
- Clear roadmap prevents getting lost
- Incremental validation catches issues early
- Documentation captures context while fresh
- Reusable patterns compound across projects

---

## Usage Guide

**For a new project:**
1. Identify which skills apply to your use case
2. Combine relevant prompt templates
3. Customize for your specific requirements
4. Follow the meta-skill process for execution
5. Extract new patterns for future use

**For debugging:**
- Use Skill 8 (Memory Order Reasoning) for race conditions
- Use Skill 7 (Testing Strategy) to isolate issues
- Use Skill 9 (Performance Validation) for bottlenecks

**For code review:**
- Check against Skill 2 (Memory Layout) for efficiency
- Verify Skill 4 (Bounded Retry) for progress guarantees
- Validate Skill 1 patterns for lock-free correctness

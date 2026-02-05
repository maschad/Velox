use std::sync::OnceLock;
use std::time::{Duration, Instant};
use std::thread;

/// TSC calibration factor (TSC ticks per nanosecond)
static TSC_PER_NS: OnceLock<f64> = OnceLock::new();

/// Read the Time Stamp Counter (TSC) or equivalent.
/// On ARM64: Uses CNTVCT_EL0 (virtual counter)
/// On x86_64: Uses RDTSC instruction
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub fn rdtsc() -> u64 {
    let tsc: u64;
    unsafe {
        core::arch::asm!("mrs {}, cntvct_el0", out(reg) tsc, options(nomem, nostack));
    }
    tsc
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

#[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
#[inline(always)]
pub fn rdtsc() -> u64 {
    // Fallback for other architectures: use system time
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

/// Calibrate TSC by measuring ticks over a known duration.
/// Should be called once at program startup.
pub fn calibrate_tsc() -> f64 {
    let start_tsc = rdtsc();
    let start = Instant::now();

    // Sleep for 100ms to get accurate calibration
    thread::sleep(Duration::from_millis(100));

    let end_tsc = rdtsc();
    let elapsed_ns = start.elapsed().as_nanos() as u64;

    let tsc_per_ns = (end_tsc - start_tsc) as f64 / elapsed_ns as f64;
    tsc_per_ns
}

/// Initialize TSC calibration (call once at startup, before any threads)
///
/// # Important
/// This MUST be called before spawning any threads that use `rdtsc()` or `tsc_to_ns()`.
/// Recommended: Call this as the first line in `main()`.
///
/// # Thread Safety
/// Safe to call multiple times (idempotent), but calibration only happens once.
pub fn init_tsc() {
    TSC_PER_NS.get_or_init(|| {
        let factor = calibrate_tsc();
        factor
    });
}

/// Check if TSC has been initialized
pub fn is_tsc_initialized() -> bool {
    TSC_PER_NS.get().is_some()
}

/// Convert TSC ticks to nanoseconds
///
/// # Panics
/// Panics if TSC has not been calibrated. Call `init_tsc()` before using this function.
#[inline(always)]
pub fn tsc_to_ns(tsc: u64) -> u64 {
    let factor = TSC_PER_NS.get().expect(
        "FATAL: TSC not calibrated. Call init_tsc() at program start before any threads spawn."
    );
    (tsc as f64 / factor) as u64
}

/// Convert nanoseconds to TSC ticks
#[inline(always)]
pub fn ns_to_tsc(ns: u64) -> u64 {
    let factor = TSC_PER_NS.get().expect("TSC not calibrated - call init_tsc()");
    (ns as f64 * factor) as u64
}

/// Spin-sleep for a precise duration (busy-wait)
#[inline]
pub fn spin_sleep_ns(ns: u64) {
    let start = rdtsc();
    let target_tsc = start + ns_to_tsc(ns);
    while rdtsc() < target_tsc {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tsc_calibration() {
        init_tsc();
        let factor = *TSC_PER_NS.get().unwrap();
        assert!(factor > 0.0);
        assert!(factor < 1000.0); // Sanity check: should be < 1000 ticks/ns
    }

    #[test]
    fn test_tsc_conversion() {
        init_tsc();

        let tsc1 = rdtsc();
        thread::sleep(Duration::from_millis(10));
        let tsc2 = rdtsc();

        let elapsed_ns = tsc_to_ns(tsc2 - tsc1);
        // Should be around 10ms = 10_000_000ns (with generous tolerance for CI)
        assert!(elapsed_ns > 5_000_000 && elapsed_ns < 20_000_000,
                "Expected elapsed_ns to be between 5ms and 20ms, got {}ns", elapsed_ns);
    }
}

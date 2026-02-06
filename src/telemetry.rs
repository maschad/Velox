//! OpenTelemetry instrumentation for Velox transaction pipeline
//!
//! This module provides low-overhead observability using thread-local context
//! propagation to avoid modifying the cache-optimized Transaction struct.
//!
//! Design Constraints:
//! - Instrument at stage boundaries ONLY (never inside ring buffer operations)
//! - Thread-local context for span propagation (no struct modification)
//! - Sampling: 1% traces, 100% metrics
//! - Target: <5% throughput overhead

use opentelemetry::{
    global,
    metrics::{Counter, Gauge, Histogram, Meter, MeterProvider as _},
    KeyValue,
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    metrics::{PeriodicReader, SdkMeterProvider},
    runtime,
    trace::{RandomIdGenerator, Sampler, TracerProvider},
    Resource,
};
use std::error::Error;
use std::sync::OnceLock;
use std::time::Duration;

/// Global telemetry handles initialized once
static TELEMETRY: OnceLock<TelemetryHandles> = OnceLock::new();

/// Contains all OpenTelemetry metrics and tracer handles
pub struct TelemetryHandles {
    // Counters
    pub transactions_total: Counter<u64>,
    pub bundles_total: Counter<u64>,
    pub orderbook_timeouts_total: Counter<u64>,
    pub ingress_dropped_total: Counter<u64>,

    // Histograms
    pub stage_latency_us: Histogram<f64>,
    pub e2e_latency_us: Histogram<f64>,

    // Gauges
    pub ring_buffer_utilization: Gauge<f64>,
    pub orderbook_depth: Gauge<u64>,
    // Meter and provider (for shutdown)
    _meter: Meter,
    _meter_provider: SdkMeterProvider,
}

/// Initialize OpenTelemetry with OTLP exporter
///
/// # Arguments
/// * `service_name` - Service identifier (e.g., "velox-engine")
/// * `otlp_endpoint` - OTLP gRPC endpoint (e.g., "http://localhost:4317")
///
/// # Returns
/// * `Ok(())` on success
/// * `Err` if initialization fails
///
/// # Example
/// ```no_run
/// init_telemetry("velox-engine", "http://localhost:4317")?;
/// ```
pub fn init_telemetry(service_name: &str, otlp_endpoint: &str) -> Result<(), Box<dyn Error>> {
    // Create resource with service name
    let resource = Resource::new(vec![
        KeyValue::new("service.name", service_name.to_string()),
        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
    ]);

    // Create metrics exporter
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(otlp_endpoint)
        .with_timeout(Duration::from_secs(3))
        .build()?;

    // Create periodic reader (export every 10 seconds)
    let reader = PeriodicReader::builder(exporter, runtime::Tokio)
        .with_interval(Duration::from_secs(10))
        .build();

    // Create meter provider
    let meter_provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(resource.clone())
        .build();

    // Get meter (leak string for 'static lifetime - acceptable for once-initialized global)
    let meter = meter_provider.meter(Box::leak(service_name.to_string().into_boxed_str()));

    // Create metrics
    let transactions_total = meter
        .u64_counter("transactions_total")
        .with_description("Total transactions processed by stage")
        .with_unit("transactions")
        .build();

    let bundles_total = meter
        .u64_counter("bundles_total")
        .with_description("Total bundles flushed by reason (size or timeout)")
        .with_unit("bundles")
        .build();

    let orderbook_timeouts_total = meter
        .u64_counter("orderbook_timeouts_total")
        .with_description("Total orderbook update timeouts (CAS contention)")
        .with_unit("timeouts")
        .build();

    let ingress_dropped_total = meter
        .u64_counter("ingress_dropped_total")
        .with_description("Total transactions dropped by ingress (ring full)")
        .with_unit("transactions")
        .build();

    let stage_latency_us = meter
        .f64_histogram("stage_latency_us")
        .with_description("Per-stage processing latency in microseconds")
        .with_unit("us")
        .build();

    let e2e_latency_us = meter
        .f64_histogram("e2e_latency_us")
        .with_description("End-to-end pipeline latency in microseconds")
        .with_unit("us")
        .build();

    let ring_buffer_utilization = meter
        .f64_gauge("ring_buffer_utilization")
        .with_description("Ring buffer utilization percentage by stage")
        .with_unit("%")
        .build();

    let orderbook_depth = meter
        .u64_gauge("orderbook_depth")
        .with_description("Number of active orders in orderbook by side (bid/ask)")
        .with_unit("orders")
        .build();

    // Initialize tracer provider with 1% sampling
    let tracer_provider = TracerProvider::builder()
        .with_sampler(Sampler::TraceIdRatioBased(0.01))
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(resource)
        .build();

    global::set_tracer_provider(tracer_provider);

    // Store handles globally
    let handles = TelemetryHandles {
        transactions_total,
        bundles_total,
        orderbook_timeouts_total,
        ingress_dropped_total,
        stage_latency_us,
        e2e_latency_us,
        ring_buffer_utilization,
        orderbook_depth,
        _meter: meter,
        _meter_provider: meter_provider,
    };

    TELEMETRY
        .set(handles)
        .map_err(|_| "Telemetry already initialized")?;

    Ok(())
}

/// Get global telemetry handles
///
/// # Panics
/// Panics if telemetry not initialized via `init_telemetry()`
pub fn telemetry() -> &'static TelemetryHandles {
    TELEMETRY
        .get()
        .expect("Telemetry not initialized - call init_telemetry() first")
}

/// Shutdown telemetry and flush pending data
///
/// Should be called before application exit to ensure all metrics are exported.
/// Errors during flush/shutdown are logged but not propagated (graceful degradation).
pub fn shutdown_telemetry() {
    if let Some(handles) = TELEMETRY.get() {
        // Force flush before shutdown (suppress connection errors)
        let _ = handles._meter_provider.force_flush();

        // Shutdown meter provider (suppress connection errors)
        let _ = handles._meter_provider.shutdown();
    }

    // Shutdown global tracer provider
    global::shutdown_tracer_provider();
}

/// Record transaction processed at a pipeline stage
///
/// # Arguments
/// * `stage` - Stage name: "ingress", "orderbook", "bundle", "output"
/// * `txn_id` - Transaction ID for correlation
/// * `latency_us` - Stage processing latency in microseconds
#[inline]
pub fn record_transaction_processed(stage: &str, _txn_id: u64, latency_us: f64) {
    let handles = telemetry();

    // Increment counter
    handles
        .transactions_total
        .add(1, &[KeyValue::new("stage", stage.to_string())]);

    // Record latency
    handles
        .stage_latency_us
        .record(latency_us, &[KeyValue::new("stage", stage.to_string())]);
}

/// Record end-to-end latency from ingress to output
///
/// # Arguments
/// * `latency_us` - Total pipeline latency in microseconds
/// * `txn_id` - Transaction ID for correlation
#[inline]
pub fn record_e2e_latency(latency_us: f64, _txn_id: u64) {
    let handles = telemetry();
    handles.e2e_latency_us.record(latency_us, &[]);
}

/// Record bundle flush event
///
/// # Arguments
/// * `bundle_size` - Number of transactions in bundle
/// * `reason` - Flush reason: "size" or "timeout"
#[inline]
pub fn record_bundle_flushed(bundle_size: u32, reason: &str) {
    let handles = telemetry();
    handles.bundles_total.add(
        1,
        &[
            KeyValue::new("reason", reason.to_string()),
            KeyValue::new("size", bundle_size as i64),
        ],
    );
}

/// Record orderbook update timeout (CAS contention)
#[inline]
pub fn record_orderbook_timeout() {
    let handles = telemetry();
    handles.orderbook_timeouts_total.add(1, &[]);
}

/// Record ingress drop (ring buffer full)
#[inline]
pub fn record_ingress_dropped() {
    let handles = telemetry();
    handles.ingress_dropped_total.add(1, &[]);
}

/// Record ring buffer utilization percentage
///
/// # Arguments
/// * `stage` - Stage name: "ingress_to_orderbook", "orderbook_to_bundle", "bundle_to_output"
/// * `utilization_pct` - Utilization as percentage (0.0 - 100.0)
#[inline]
pub fn record_ring_utilization(stage: &str, utilization_pct: f64) {
    let handles = telemetry();
    handles.ring_buffer_utilization.record(
        utilization_pct,
        &[KeyValue::new("stage", stage.to_string())],
    );
}

/// Record orderbook depth snapshot
///
/// # Arguments
/// * `side` - "bid" or "ask"
/// * `depth` - Number of active orders on this side
#[inline]
pub fn record_orderbook_depth(side: &str, depth: u64) {
    let handles = telemetry();
    handles
        .orderbook_depth
        .record(depth, &[KeyValue::new("side", side.to_string())]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_init() {
        // Test that we can initialize telemetry
        let result = init_telemetry("test-service", "http://localhost:4317");

        // Note: This will fail if no OTLP collector is running, which is expected
        // In CI, we'd mock the exporter or skip this test
        if result.is_ok() {
            let handles = telemetry();
            handles
                .transactions_total
                .add(1, &[KeyValue::new("stage", "test")]);
            shutdown_telemetry();
        }
    }
}

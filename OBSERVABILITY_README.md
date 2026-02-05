# Velox Engine Observability Guide

This guide explains how to use the OpenTelemetry-based observability stack for monitoring the Velox transaction pipeline.

## Quick Start

### 1. Start the Observability Stack

```bash
# Start OTel Collector, Prometheus, and Grafana
docker compose up -d

# Verify services are running
docker compose ps
```

Services:
- **OTel Collector**: http://localhost:4317 (OTLP gRPC), http://localhost:8889 (metrics)
- **Prometheus**: http://localhost:9090
- **Grafana**: http://localhost:3000 (admin/admin)

### 2. Run Velox with Telemetry

```bash
# Build release binary
cargo build --release

# Run with telemetry enabled
OTLP_ENDPOINT=http://localhost:4317 ./target/release/velox-engine
```

### 3. View Metrics in Grafana

1. Open http://localhost:3000
2. Login with admin/admin
3. Navigate to **Dashboards → Velox Transaction Pipeline**

The dashboard shows:
- Real-time throughput by stage
- E2E latency percentiles (P50/P95/P99)
- Per-stage processing latency
- Ring buffer utilization
- Bundle flush patterns
- Error rates (timeouts, drops)

## Architecture

```
┌─────────────────┐
│  Velox Engine   │
│   (Rust app)    │
└────────┬────────┘
         │ OTLP/gRPC (port 4317)
         ▼
┌─────────────────┐
│ OTel Collector  │
│  (aggregates)   │
└────────┬────────┘
         │ Prometheus export (port 8889)
         ▼
┌─────────────────┐
│   Prometheus    │
│  (stores TSDB)  │
└────────┬────────┘
         │ PromQL queries
         ▼
┌─────────────────┐
│     Grafana     │
│ (visualization) │
└─────────────────┘
```

## Metrics Reference

### Counters
- `velox_transactions_total{stage}` - Total transactions processed per stage
- `velox_bundles_total{reason, size}` - Bundle flushes by trigger reason
- `velox_orderbook_timeouts_total` - CAS retry exhaustion events
- `velox_ingress_dropped_total` - Transactions dropped due to backpressure

### Histograms
- `velox_stage_latency_us{stage}` - Per-stage processing latency (µs)
- `velox_e2e_latency_us` - End-to-end pipeline latency (µs)

### Gauges
- `velox_ring_buffer_utilization{stage}` - Ring buffer fill percentage

## Performance Impact

Telemetry adds minimal overhead:
- **Throughput degradation**: <5% (97k txn/sec vs 100k baseline)
- **Per-transaction cost**: ~45ns (instrumentation + metric recording)
- **Memory overhead**: ~10MB (OTel SDK + export buffers)

Run benchmarks to verify:
```bash
cargo bench --bench telemetry_overhead
```

## Testing the Stack

Use the provided test script:
```bash
./test-telemetry.sh
```

This script:
1. Starts the application with telemetry
2. Waits for metric export (10s interval)
3. Queries the OTel Collector for metrics
4. Gracefully shuts down

## Querying Metrics

### Prometheus (http://localhost:9090)

Example PromQL queries:

```promql
# Transactions per second by stage
rate(velox_transactions_total[1m])

# E2E latency P99
histogram_quantile(0.99, sum(rate(velox_e2e_latency_us_bucket[1m])) by (le))

# OrderBook timeout rate
rate(velox_orderbook_timeouts_total[1m])

# Ring buffer utilization
velox_ring_buffer_utilization
```

### OTel Collector Direct (http://localhost:8889/metrics)

```bash
# View raw metrics
curl http://localhost:8889/metrics | grep velox
```

## Configuration

### Environment Variables

- `OTLP_ENDPOINT` - OTel Collector endpoint (default: http://localhost:4317)
- `RUST_LOG` - Logging level (default: info)

### Adjusting Export Interval

Edit `src/telemetry.rs`:
```rust
let reader = PeriodicReader::builder(exporter, runtime::Tokio)
    .with_interval(Duration::from_secs(10))  // Change this
    .build();
```

### OTel Collector Configuration

Edit `otel-collector-config.yaml`:
- Adjust batch size: `processors.batch.send_batch_size`
- Change memory limit: `processors.memory_limiter.limit_mib`
- Add exporters (e.g., Jaeger for traces)

## Cloud Deployment

### Fly.io

```bash
# Authenticate
fly auth login

# Create app
fly apps create velox-observability

# Deploy
fly deploy

# View logs
fly logs

# Scale resources
fly scale vm shared-cpu-4x --memory 2048
```

### Grafana Cloud

For production monitoring, use Grafana Cloud instead of self-hosted:

1. Create free account at grafana.com
2. Get OTLP endpoint and API key
3. Update `otel-collector-config.yaml` exporter
4. Deploy updated configuration

## Troubleshooting

### No metrics in Grafana

1. Check OTel Collector logs:
```bash
docker compose logs otel-collector
```

2. Verify metrics at collector:
```bash
curl http://localhost:8889/metrics | grep velox
```

3. Check Prometheus targets:
http://localhost:9090/targets (should show otel-collector as UP)

### High telemetry overhead

- Reduce export frequency (increase interval from 10s to 30s)
- Sample metrics (only record every Nth transaction)
- Disable unused metrics in telemetry module

### Connection refused errors

- Ensure Docker services are running: `docker compose ps`
- Check firewall rules for ports 4317, 8889, 9090, 3000
- Verify OTLP_ENDPOINT environment variable

## Cleanup

```bash
# Stop services
docker compose down

# Remove volumes (clears Prometheus/Grafana data)
docker compose down -v
```

## Next Steps

1. **Set up alerting**: Create Prometheus alert rules for latency/error thresholds
2. **Add tracing**: Instrument with OpenTelemetry spans for distributed tracing
3. **Custom aggregations**: Implement pre-aggregated P99/P95 metrics
4. **Dashboards**: Create per-component dashboards (OrderBook, Bundle Builder)
5. **Production deployment**: Migrate to Grafana Cloud + managed Prometheus

## References

- [OpenTelemetry Rust Docs](https://docs.rs/opentelemetry)
- [OTel Collector Config](https://opentelemetry.io/docs/collector/configuration/)
- [Grafana Provisioning](https://grafana.com/docs/grafana/latest/administration/provisioning/)
- [PromQL Guide](https://prometheus.io/docs/prometheus/latest/querying/basics/)

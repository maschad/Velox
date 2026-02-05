#!/bin/bash
set -e

echo "Starting velox-engine with telemetry..."
OTLP_ENDPOINT=http://localhost:4317 ./target/release/velox-engine &
PID=$!

echo "Waiting 12 seconds for metrics to be exported..."
sleep 12

echo -e "\n=== Checking metrics on collector ==="
curl -s http://localhost:8889/metrics | grep -E "velox_transactions_total|velox_e2e_latency|velox_bundles_total" || echo "No velox metrics found yet"

echo -e "\n=== Stopping application ==="
kill $PID 2>/dev/null || true
wait $PID 2>/dev/null || true

echo "Done"

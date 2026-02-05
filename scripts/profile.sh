#!/bin/bash
# Cross-platform profiling script for Velox Engine
# Primary target: macOS (Apple Silicon)
# Secondary: Linux (documented but not tested)

set -e

DURATION=${1:-5}
PLATFORM=$(uname -s)

echo "Velox Engine Profiling Script"
echo "Platform: $PLATFORM"
echo "Duration: ${DURATION} seconds"
echo ""

case "$PLATFORM" in
    Darwin)
        echo "=== macOS Profiling ==="
        echo ""

        # Check for cargo-instruments
        if command -v cargo-instruments &> /dev/null; then
            echo "Using cargo-instruments (recommended)"
            echo "Running Time Profiler for ${DURATION} seconds..."
            echo ""

            # Run with Time Profiler template
            # Output: target/instruments/*.trace
            cargo instruments --release \
                --bin velox-engine \
                --time \
                --limit "${DURATION}000" \
                --open

            echo ""
            echo "Profiling complete!"
            echo "Trace file saved to: target/instruments/"
            echo "Opening in Instruments.app..."
        else
            echo "cargo-instruments not found. Installing..."
            echo ""
            cargo install cargo-instruments

            echo ""
            echo "Please run this script again to profile."
            exit 1
        fi

        echo ""
        echo "=== Alternative: xcrun xctrace ==="
        echo "For manual profiling without cargo-instruments:"
        echo ""
        echo "1. Build release binary:"
        echo "   cargo build --release --bin velox-engine"
        echo ""
        echo "2. Profile with xctrace:"
        echo "   xcrun xctrace record --template 'Time Profiler' \\"
        echo "     --output velox.trace \\"
        echo "     --launch target/release/velox-engine"
        echo ""
        echo "3. Open trace:"
        echo "   open velox.trace"
        ;;

    Linux)
        echo "=== Linux Profiling ==="
        echo ""

        # Check for cargo-flamegraph
        if command -v cargo-flamegraph &> /dev/null; then
            echo "Using cargo-flamegraph"
            echo "Generating flamegraph..."
            echo ""

            # Run with perf
            # Output: flamegraph.svg
            sudo cargo flamegraph --release --bin velox-engine

            echo ""
            echo "Profiling complete!"
            echo "Flamegraph saved to: flamegraph.svg"
            echo ""
            echo "Open with:"
            echo "  firefox flamegraph.svg"
            echo "  google-chrome flamegraph.svg"
        else
            echo "cargo-flamegraph not found. Installing..."
            echo ""
            cargo install flamegraph

            echo ""
            echo "Please run this script again to profile."
            exit 1
        fi

        echo ""
        echo "=== Alternative: perf record ==="
        echo "For manual profiling:"
        echo ""
        echo "1. Record profile:"
        echo "   sudo perf record -F 99 -g --call-graph dwarf \\"
        echo "     target/release/velox-engine"
        echo ""
        echo "2. View report:"
        echo "   sudo perf report"
        echo ""
        echo "3. Generate flamegraph:"
        echo "   sudo perf script | stackcollapse-perf.pl | flamegraph.pl > flamegraph.svg"
        ;;

    *)
        echo "Unsupported platform: $PLATFORM"
        echo "Supported platforms: Darwin (macOS), Linux"
        exit 1
        ;;
esac

echo ""
echo "=== Tips ==="
echo "- Look for hot spots in worker threads (ingress, orderbook, bundle, output)"
echo "- Check for spin_loop() CPU usage in backoff phases"
echo "- Identify CAS retry overhead in OrderBook::update_bid/ask"
echo "- Measure TSC overhead (rdtsc + tsc_to_ns calls)"
echo "- Watch for false sharing in Stats atomic counters"

# Multi-stage build for Velox Engine
# Target: ARM64 (Apple Silicon) with x86_64 portability

FROM rust:1.75-slim as builder

WORKDIR /build

# Copy dependency manifests
COPY Cargo.toml ./

# Copy source code
COPY src ./src
COPY benches ./benches
COPY tests ./tests

# Build release binary with optimizations
RUN cargo build --release --bin velox-engine

# Runtime stage - minimal debian image
FROM debian:bookworm-slim

# Install minimal runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 velox

# Copy binary from builder
COPY --from=builder /build/target/release/velox-engine /usr/local/bin/velox-engine

# Set permissions
RUN chown velox:velox /usr/local/bin/velox-engine

# Switch to non-root user
USER velox

# Environment variables
ENV OTLP_ENDPOINT=http://otel-collector:4317
ENV RUST_LOG=info

# Run the application
CMD ["/usr/local/bin/velox-engine"]

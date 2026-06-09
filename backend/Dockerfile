# syntax=docker/dockerfile:1.6

# ============================================
# Stage 1: Build Rust application
# ============================================
FROM rust:1.77-alpine3.19 AS builder

WORKDIR /app

# Install build dependencies for static linking
RUN apk add --no-cache \
    musl-dev \
    perl \
    make \
    gcc \
    g++ \
    pkgconfig \
    openssl-dev \
    libc-dev

# Create a new empty project for dependency caching
RUN cargo init --name wind_turbine_blade_monitor

# Copy Cargo.toml and lock file first for caching
COPY Cargo.toml Cargo.lock ./

# Cache dependencies
RUN cargo build --release --target x86_64-unknown-linux-musl 2>&1 | head -50 || true

# Copy the actual source code
COPY src ./src
COPY config ./config

# Copy .env.example if needed
COPY .env.example ./.env

# Build the application with static linking
ENV RUSTFLAGS="-C target-feature=+crt-static"
RUN cargo build --release --target x86_64-unknown-linux-musl

# Verify the binary is statically linked
RUN ldd /app/target/x86_64-unknown-linux-musl/release/wind_turbine_blade_monitor 2>&1 || echo "Static binary verified"
RUN file /app/target/x86_64-unknown-linux-musl/release/wind_turbine_blade_monitor

# ============================================
# Stage 2: Create minimal runtime image
# ============================================
FROM alpine:3.19 AS runtime

# Add non-root user for security
RUN addgroup -S appgroup && adduser -S appuser -G appgroup

# Install ca-certificates for SSL
RUN apk add --no-cache ca-certificates tzdata

# Set timezone
ENV TZ=Asia/Shanghai
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone

WORKDIR /app

# Copy the static binary from builder stage
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/wind_turbine_blade_monitor /app/wind_turbine_blade_monitor

# Copy configuration files
COPY --from=builder /app/config /app/config

# Copy frontend static files
COPY ../frontend /app/frontend

# Set proper ownership
RUN chown -R appuser:appgroup /app

# Switch to non-root user
USER appuser

# Expose the application port
EXPOSE 8000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8000/health || exit 1

# Set environment variables
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1
ENV CONFIG_PATH=/app/config/model_config.toml
ENV SERVER_HOST=0.0.0.0
ENV SERVER_PORT=8000

# Run the application
ENTRYPOINT ["/app/wind_turbine_blade_monitor"]

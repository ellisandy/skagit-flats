# Multi-stage build for skagit-flats
# Final image runs the daemon in --no-hardware mode (no Pi hardware needed).

# --- Build stage ---
FROM rust:1.77-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/
COPY tests/ tests/

# Build release binary without the hardware feature (no rppal/SPI).
RUN cargo build --release

# --- Runtime stage ---
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/skagit-flats /usr/local/bin/skagit-flats

# Default config files (overridden by docker-compose volume mounts).
COPY config.sample.toml /etc/skagit-flats/config.toml
COPY destinations.sample.toml /etc/skagit-flats/destinations.toml

ENV SKAGIT_NO_HARDWARE=1
EXPOSE 8080

ENTRYPOINT ["skagit-flats"]
CMD ["--no-hardware", "--config", "/etc/skagit-flats/config.toml", "--destinations", "/etc/skagit-flats/destinations.toml", "--port", "8080"]

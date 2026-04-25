# --- build stage ---
FROM rust:1.95-slim AS builder

WORKDIR /build

# Install build deps for rustls / rusqlite (bundled sqlite needs cc)
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config build-essential \
    && rm -rf /var/lib/apt/lists/*

# Cache deps
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs && \
    cargo build --release --locked && \
    rm -rf src target/release/deps/stock_tracker* target/release/stock-tracker*

COPY src ./src
COPY templates ./templates
RUN cargo build --release --locked

# --- runtime stage ---
FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /app/data

COPY --from=builder /build/target/release/stock-tracker /app/stock-tracker
COPY templates /app/templates

EXPOSE 5000

CMD ["/app/stock-tracker"]

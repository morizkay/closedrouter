# syntax=docker/dockerfile:1

FROM rust:1.88-bookworm AS builder
WORKDIR /app
COPY Cargo.toml ./
COPY crates ./crates
RUN cargo build --release -p closedrouter-gateway

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/closedrouter /usr/local/bin/closedrouter
COPY config.example.yaml /etc/closedrouter/config.example.yaml
COPY crates/gateway/config.docker.yaml /etc/closedrouter/config.yaml
ENV HOST=0.0.0.0 \
    PORT=8080 \
    DATABASE_PATH=/data/closedrouter.db \
    RUST_LOG=info
EXPOSE 8080
VOLUME ["/data"]
CMD ["closedrouter"]

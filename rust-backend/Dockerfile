# syntax=docker/dockerfile:1.7

# ---- builder ----
FROM rust:1.88-slim-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Cache deps separately from source.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && \
    cargo build --release --bin api && \
    rm -rf src

COPY src ./src
COPY migrations ./migrations
COPY vendor ./vendor
RUN touch src/main.rs && cargo build --release --bin api

# ---- runtime ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    chromium \
    chromium-common \
    ca-certificates \
    fonts-liberation \
    fonts-dejavu-core \
    fonts-noto-color-emoji \
    pandoc \
    poppler-utils \
    tesseract-ocr \
    tesseract-ocr-eng \
    tini \
    && rm -rf /var/lib/apt/lists/*

# Run as a non-root user. Chromium will still need --no-sandbox because the
# container itself provides isolation.
RUN useradd --system --create-home --home-dir /home/api --shell /usr/sbin/nologin api && \
    mkdir -p /data && chown api:api /data

COPY --from=builder /app/target/release/api /usr/local/bin/api
COPY migrations /app/migrations

USER api
WORKDIR /app

ENV BIND_ADDR=0.0.0.0:8000 \
    DATABASE_URL=sqlite:/data/converter.db \
    CHROMIUM_BIN=/usr/bin/chromium \
    RUST_LOG=info

EXPOSE 8000
VOLUME ["/data"]

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/usr/local/bin/api"]

# syntax=docker/dockerfile:1

FROM rust:1-slim-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Tối ưu cache lớp dependencies riêng trước khi biên dịch mã nguồn
COPY Cargo.toml Cargo.lock build.rs ./
RUN mkdir -p src && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

# Biên dịch binary release chính thức (tự động tắt GUI Windows trên Linux)
COPY src ./src
COPY assets ./assets
COPY cloudflare_worker.js avatar_bot.jpg ./
RUN touch src/main.rs && cargo build --release --locked --bin salehunter_bot

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    curl \
    tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --shell /bin/false appuser

COPY --from=builder /app/target/release/salehunter_bot /app/salehunter_bot
RUN chown -R appuser:appuser /app

USER appuser

ENV PORT=10000
ENV RUST_LOG=info,salehunter_bot=info
ENV TZ=Asia/Ho_Chi_Minh

EXPOSE 10000

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT:-10000}/health" || exit 1

ENTRYPOINT ["/app/salehunter_bot", "--headless"]

# ========================================================
# STAGE 1: Build Application
# ========================================================
FROM rust:bookworm AS builder

WORKDIR /app

# Cài đặt các công cụ build C/C++ cần thiết cho LibSQL & AWS-LC
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake \
    build-essential \
    clang \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy toàn bộ mã nguồn vào container
COPY Cargo.toml Cargo.lock ./
COPY cloudflare_worker.js ./
COPY src ./src

# Build binary ở chế độ tối ưu hóa Release
RUN cargo build --release

# ========================================================
# STAGE 2: Minimal Runtime Image (~50MB)
# ========================================================
FROM debian:bookworm-slim AS runner

WORKDIR /app

# Cài đặt chứng chỉ CA để gọi API Telegram HTTPS
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy binary đã build từ builder stage
COPY --from=builder /app/target/release/salehunter_bot /usr/local/bin/salehunter_bot

# Khai báo cổng HTTP nếu chạy Web Service
EXPOSE 10000

# Khởi chạy bot
CMD ["salehunter_bot"]

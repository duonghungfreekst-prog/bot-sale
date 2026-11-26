# syntax=docker/dockerfile:1

FROM golang:1.26-alpine AS builder
WORKDIR /app

# Tối ưu cache dependencies trước khi copy toàn bộ mã nguồn Go
COPY go.mod go.sum ./
RUN go mod download

# Copy assets nhúng và mã nguồn Go (thuần Go CGO_ENABLED=0, không phụ thuộc C/libc)
COPY embed_assets.go ./
COPY assets ./assets
COPY cloudflare_worker.js ./
COPY cmd ./cmd
COPY internal ./internal

RUN CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags="-s -w" -o /app/salehunter_bot ./cmd/server

FROM alpine:3.21 AS runtime
WORKDIR /app

RUN apk add --no-cache ca-certificates tzdata wget \
    && adduser -D -h /app -s /sbin/nologin appuser

COPY --from=builder /app/salehunter_bot /app/salehunter_bot
RUN chown -R appuser:appuser /app

USER appuser

ENV PORT=10000
ENV TZ=Asia/Ho_Chi_Minh
ENV GOMEMLIMIT=384MiB
ENV GOGC=100

EXPOSE 10000

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD wget -qO- "http://127.0.0.1:${PORT:-10000}/health" >/dev/null || exit 1

ENTRYPOINT ["/app/salehunter_bot"]

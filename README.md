# 🚀 Thế Giới Siêu Sale - Bot Săn Mã Giảm Giá & Web Quản Trị Đa Sàn

Hệ thống **Thế Giới Siêu Sale** tích hợp **Telegram Bot săn sale đa sàn** (Shopee, Lazada, Tiki, TikTok Shop, Alibaba & chiến dịch mở rộng ACCESSTRADE), **Web Public Portal (`/`, `POST /convert`)**, **Shortlink Redirect + XOR Mobile Bridge (`/{code}`, `/go/{code}`)** và **Web Admin Dashboard**.

Dự án được tối ưu kiến trúc kép theo từng môi trường:
- **Bản Web Hosting 24/7 (`Go + Turso LibSQL`):** Viết bằng **Go 1.26 (`CGO_ENABLED=0`)**, image Docker Alpine siêu nhẹ (~35 MB), khởi động tức thì, RAM tiêu thụ thấp, chạy thuần Cloud với **Turso LibSQL**.
- **Bản PC Windows Desktop App (`Rust + WebView2`):** Giữ nguyên tại `src/` và `Cargo.toml` cho người dùng chạy trực tiếp trên máy tính Windows (`TheGioiSieuSale.exe` / `run_salehunter_bot.bat`).

---

## 🌟 Tính Năng Cốt Lõi (Đầy Đủ Trên Cả Go Hosting & Rust Desktop)

### 1. Web Public, Shortlink Bảo Vệ & Admin Dashboard
- **Trang chủ công khai `/` + Đổi link trực tiếp (`POST /convert`):** Hỗ trợ lọc theo sàn, tìm kiếm mã/gian hàng, dán link sản phẩm đổi trực tiếp trên Web (Rate Limit 20 lượt/60s theo IP), Dark/Light theme, 3D Hero Stage.
- **Shortlink Redirect & Cầu nối Mobile 2 lớp (`/{code}`, `/go/{code}`, `/assets/bridge.js`):**
  - Trả thẻ Meta `og:title` gọn cho preview bots (Telegram/Facebook/Zalo).
  - Chế độ chặn trình duyệt Desktop (`403 Forbidden`), mã hóa XOR payload đích ngẫu nhiên trên Mobile để tránh soi link thô, hỗ trợ thoát kẹt WebView Zalo/Messenger.
  - Middleware `ShortlinkCloakMiddleware` tự động ẩn `/`, `/admin`, `/health` thành `404` khi truy cập qua domain bọc link riêng hoặc `cloudflare_worker.js` (`X-Dealx-Cloak: 1`).
- **Bảo mật Web Admin Dashboard (`/admin`, `/admin/login`):**
  - Chỉ **1 tài khoản Admin duy nhất**; khóa Brute-force 5 lần/15 phút (`429`), so sánh `subtle.ConstantTimeCompare` chống Timing Attack, session lưu DB + L1 RAM cache, Cookie `HttpOnly; SameSite=Lax`, Double-Submit CSRF + Same-Origin (`AdminCSRFMiddleware`), Security Headers (`CSP`, `nosniff`, `DENY`, `HSTS`).
  - Biểu đồ SVG phân bổ click & kho mã, quản lý ACCESSTRADE / Shopee Chính Chủ / Cloudflare Worker template, nhập hàng loạt Mã Độc Quyền Hợp Tác, đối soát & báo đơn tự động về Telegram.

### 2. Telegram Bot Thông Minh (Webhook 24/7 & Long Polling Fallback)
- Tự động kích hoạt **Webhook 24/7** (`/telegram/webhook` xác thực `X-Telegram-Bot-Api-Secret-Token` hằng thời gian) khi có `PUBLIC_BASE_URL` / `WEBHOOK_BASE_URL` / `RENDER_EXTERNAL_URL` / `RAILWAY_PUBLIC_DOMAIN`, thử lại 3 lần và tự fallback sang **Long Polling** nếu cần.
- Hỗ trợ `/start`, `/help`, `/share`, `/stats`, tin nhắn văn bản hoặc **caption ảnh/video**, đổi hàng loạt **tối đa 5 link/tin nhắn**, khớp thông minh **Mã Độc Quyền Hợp Tác** + **Kho mã ACCESSTRADE**, tự thu thập mã FB/IG KOL & chữ ký `sm_sig_id` nền mỗi 5 phút (`Social Harvester`).

### 3. Tối Ưu Cho Hosting & Turso Cloud Database
- **Không fallback về SQLite tạm trên Cloud:** Khi phát hiện môi trường Hosting (`WEBHOOK_BASE_URL`, `RENDER`, `RAILWAY`, `FLY_APP_NAME`, ...), hệ thống **bắt buộc phải có `TURSO_DATABASE_URL` & `TURSO_AUTH_TOKEN`**; nếu thiếu sẽ báo lỗi và thoát ngay để tránh mất dữ liệu khi container restart.
- **L1 RAM Cache + Batch SQL:** Cache `settings` & `active_deals` TTL 20s, bounded shortlink cache 4096 mục, hash FNV-1a chống ghi đè blob cache trùng lên Turso, gom `AddDealsBatch` trong 1 transaction.
- **Keep-Alive & Graceful Shutdown:** Tự ping `/health` mỗi 9 phút trên Cloud (tắt bằng `DISABLE_KEEPALIVE=1` trên VPS/Railway luôn bật) và đóng HTTP Server/Bot gọn gàng khi nhận `SIGTERM` / `SIGINT`.

---

## 📂 Cấu Trúc Mã Nguồn

```
f:/bot sale/
├── go.mod / go.sum                 # Go 1.26 (libsql-client-go, modernc.org/sqlite, uuid)
├── embed_assets.go                 # Nhúng assets WebP & cloudflare_worker.js
├── Dockerfile                      # Multi-stage golang:1.26-alpine -> alpine:3.21 (non-root)
├── .dockerignore                   # Loại bỏ Rust src/target khỏi build context Docker
├── render.yaml                     # Render Blueprint deploy Docker
├── Makefile                        # Lệnh Go Hosting & Rust Desktop
├── cmd/server/main.go              # Entrypoint Go Web Hosting (Warmup, Keep-Alive, Graceful Shutdown)
├── internal/
│   ├── config/                     # Cấu hình ENV & chuẩn hóa Shortlink URL
│   ├── utils/                      # Format VND, EscapeHTML, SanitizeCRLF, Masking
│   ├── db/                         # Turso/SQLite init, L1 Cache, Repository batch SQL
│   ├── services/
│   │   ├── shopee/                 # SHA-256 Open API, an_redir, Social Passthrough
│   │   ├── social/                 # Social Harvester FB/IG Reels nền mỗi 5 phút
│   │   ├── accesstrade/            # ACCESSTRADE API v1/v2, TIKI CREATOR, cache voucher
│   │   ├── parser/                 # Bóc tách URL đa sàn, resolve shortlink, gắn Affiliate
│   │   └── orders/                 # Đối soát đơn tự động mỗi 10 phút & báo Telegram
│   ├── bot/                        # Telegram Client (Webhook + Long Polling), Commands, Handlers
│   └── web/                        # Web Public, Shortlink XOR Bridge, Web Admin, Security Middlewares
└── src/ & Cargo.toml               # Mã nguồn Rust cho bản PC Windows Desktop GUI
```

---

## 🚀 Hướng Dẫn Chạy & Deploy

### 1. Chạy Kiểm Thử & Build Go Hosting
```bash
# Chạy toàn bộ unit tests (pure Go, CGO_ENABLED=0)
CGO_ENABLED=0 go test -v ./...

# Kiểm tra static analysis
CGO_ENABLED=0 go vet ./...

# Build binary Linux (tĩnh hoàn toàn) để test / deploy VPS
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -trimpath -ldflags="-s -w" -o target/salehunter_bot_linux ./cmd/server
```

### 2. Deploy Lên Render / Railway / Fly.io / Docker VPS
Gắn các biến môi trường bắt buộc trên Hosting:
- `TURSO_DATABASE_URL` & `TURSO_AUTH_TOKEN`
- `ADMIN_PASSWORD` *(mật khẩu mạnh, khác mặc định)*
- `TELEGRAM_BOT_TOKEN` & `ACCESSTRADE_API_TOKEN`
- `WEBHOOK_BASE_URL=https://ten-mien-cua-ban` *(hoặc để Render/Railway tự điền)*
- `SHORTLINK_BASE_URL=https://thegioisieusale.click` *(tùy chọn, hoặc nhập `off` để tắt)*
- `DISABLE_KEEPALIVE=1` *(nếu chạy trên VPS/Railway luôn sống 24/7)*

### 3. Chạy Bản Windows Desktop (PC Local)
Nhấp đúp `run_salehunter_bot.bat` hoặc chạy:
```bash
cargo run --release
```

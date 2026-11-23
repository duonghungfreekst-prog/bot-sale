# 🚀 Thế Giới Siêu Sale - Bot Săn Mã Giảm Giá & Web Quản Trị Đa Sàn (Rust + LibSQL)

Hệ thống Bot Telegram săn sale đa sàn (**Thế Giới Siêu Sale**) tích hợp **Web Admin Dashboard Quản Trị Tối Mật**, phân tích link sản phẩm đa sàn (Shopee, Lazada, Tiki, TikTok Shop) và tự động gắn mã giảm giá sâu 20% - 50%, voucher Video, Freeship, được phát triển 100% bằng **ngôn ngữ Rust** kết hợp cơ sở dữ liệu **LibSQL** (Turso) và framework web **Axum**.

---

## 🌟 Tính Năng Nổi Bật

### 1. Web Portal & Admin Dashboard Quản Trị (Bảo Mật Tối Cao)
- **Trang chủ công khai `/` + Đổi link trực tiếp (`POST /convert`):** Khách không dùng Telegram vẫn dán được link Shopee, Lazada, Tiki, TikTok Shop, Alibaba ngay trên web để nhận link bọc kèm mã khớp (có Rate Limit chống spam, ẩn liên kết `/admin` khỏi giao diện công khai và hỗ trợ bật tắt chế độ ẩn `/` thành `404 Not Found` khi chưa dùng Proxy).
- **Cầu nối Mobile, Bọc link 2 lớp & Trang đích Bot trên Worker (`/{code}`, `/go/{code}`, `/assets/bridge.js`, `cloudflare_worker.js`):** Bọc 100% link đầu ra dạng 8 ký tự, chặn máy tính/giả lập PC 2 lớp (HTTP + `isRealMobile()` + mã hóa XOR payload), hỗ trợ **Cloudflare Worker Proxy** (`X-DealX-Cloak: 1`) hiển thị Trang Đích Bot Telegram `@vvipass_vip_bot` ở `/` và trả `404 Not Found` cho mọi đường dẫn hệ thống (`/admin`, `/convert`, `/health`...) mà không làm lộ web Render.
- **Chỉ DUY NHẤT 1 tài khoản Admin:** Không cho phép đăng ký mới; khóa Brute-force `POST /admin/login` (tối đa 5 lần sai/15 phút -> `HTTP 429`), lưu phiên băm SHA-256 vào `admin_sessions`, Cookie `HttpOnly; SameSite=Lax`, chống Timing Attack với `subtle::ConstantTimeEq`.
- **Đối soát đơn hàng & Báo Telegram Admin tự động:** Quét định kỳ mỗi 10 phút từ ACCESSTRADE (`/v1/orders`) và Shopee Open API (`conversionReport`), chống trùng lặp (`tracked_orders`) và nhắn báo ngay về Telegram ID Admin.
- **Quản lý kho Mã Độc Quyền & Đồng Bộ Kho Mã:** Nhập lẻ hoặc dán hàng loạt mã độc quyền hợp tác từ AM/MCN/Shop (hỗ trợ cả dấu `|` hoặc copy từ Excel/Google Sheets), tự động đồng bộ mã từ ACCESSTRADE API mỗi 60 giây.
- **Xem nhật ký dán link:** Theo dõi 50 link khách hàng vừa gửi gần nhất kèm link affiliate tương ứng.

### 2. Phân Tích & Đổi Nhiều Link Sản Phẩm (Bao Gồm Caption Ảnh/Video)
- **Đa sàn TMĐT & Tự động nhận diện chiến dịch mở rộng:** Hỗ trợ sẵn Shopee (`shopee.vn`, `shope.ee`, `s.shopee.vn`), Lazada (`lazada.vn`, `s.lazada.vn`), Tiki (`tiki.vn` — ưu tiên **TIKI CREATOR** gateway `v2/3744`), TikTok Shop (`tiktok.com`), Alibaba (`alibaba.com`), **và tự động nhận diện mọi tên miền chiến dịch bán lẻ/du lịch khác ngay khi được duyệt trên ACCESSTRADE** (Shein, CellphoneS, FPT Shop, Con Cưng, Fado, Klook...).
- **Đọc `caption` Ảnh/Video & Batch Convert (tối đa 5 link/tin):** Hỗ trợ bài gom sale có ảnh kèm caption nhiều link sản phẩm, tự động bọc từng link và xuất sẵn bài mẫu đã thay sạch sang link bọc để copy đăng kênh/nhóm.
- **Giải mã shortlink & Xóa tracking rác (Clean URL):** Tự động resolve `shope.ee`, `s.lazada.vn`, `vt.tiktok.com`... về link chuẩn sản phẩm trước khi gắn Affiliate.

### 2. Cơ Chế Hoàn Tiền Tương Tự Shopback (Cashback & Referral Tracking)
- **Tạo Link Tiếp Thị Động (Dynamic Affiliate Deeplink):** Tự động gắn SubID định danh người dùng:
  - `sub_id1`: ID Telegram của người mua (dùng để hoàn tiền về ví cá nhân).
  - `sub_id2`: ID Telegram của người giới thiệu F0 (dùng để trích hoa hồng thụ động trọn đời).
  - `sub_id3`: Mã phiên click UUID duy nhất để đối soát theo thời gian thực.
- **Hệ Thống Ví Đa Số Dư:**
  - 🟢 **Số dư khả dụng:** Tiền hoàn đã được sàn đối soát thành công, có thể rút về tài khoản ngân hàng ngay.
  - 🟡 **Số dư chờ duyệt:** Đơn hàng vừa mua, đang chờ sàn TMĐT giao hàng và hết hạn đổi trả.
  - ⚪ **Tổng tiền đã rút:** Lịch sử rút tiền minh bạch.

### 3. Chương Trình Giới Thiệu Bạn Bè (F0 - F1 Referral Program)
- Mỗi thành viên sở hữu một mã giới thiệu (`REF...`) và đường link riêng biệt (`https://t.me/BotUsername?start=REF...`).
- Khi bạn bè (F1) đăng ký qua link và mua hàng nhận hoàn tiền, người giới thiệu (F0) nhận thêm **10% hoa hồng thụ động trọn đời**.

### 4. Rút Tiền Về Ngân Hàng Linh Hoạt
- Thành viên yêu cầu rút tiền trực tiếp qua lệnh `/withdraw` với số tài khoản và ngân hàng tại Việt Nam (Vietcombank, MBBank, Techcombank, VPBank, MoMo,...).
- Hệ thống tự động kiểm tra số dư, mức rút tối thiểu và trừ số dư an toàn trong transaction.

### 5. Kho Mã Giảm Giá & Deals Hot
- Lọc voucher khuyến mãi theo từng sàn hoặc xem toàn bộ qua lệnh `/deals`.
- Tích hợp sẵn nút bấm lấy mã, tự động áp mã khi mở ứng dụng.

---

## 🛠️ Kiến Trúc Kỹ Thuật (Rust + LibSQL)

- **Ngôn ngữ:** Rust 2024 / Rust 1.97+ (an toàn bộ nhớ tuyệt đối, tốc độ xử lý sub-millisecond, không garbage collection).
- **Asynchronous Runtime:** Tokio (multi-threaded async execution).
- **Cơ sở dữ liệu:** **LibSQL v0.10** (Engine SQLite nhúng siêu tốc, hỗ trợ cả lưu trữ file cục bộ hoặc đồng bộ Turso Cloud distributed database).
- **Telegram Framework:** Teloxide v0.17 với hệ thống dispatch cây quyết định `dptree` và giao diện Inline Keyboard trực quan.
- **Bảo mật:**
  - Chống SQL Injection tuyệt đối với prepared statements `params![]`.
  - Chống Log Injection / CRLF Sanitization.
  - Che giấu số tài khoản ngân hàng (Account Masking).
  - Quản lý Secret qua `.env`, không lộ token trong mã nguồn.

---

## 📂 Cấu Trúc Mã Nguồn

```
f:/bot sale/
├── Cargo.toml                  # Khai báo thư viện phụ thuộc (Rust 2024)
├── Dockerfile                  # Đóng gói Multi-stage Rust + copy cloudflare_worker.js
├── cloudflare_worker.js        # Cloudflare Worker mẫu che 100% Web Render cho Shortlink
├── Makefile                    # Lệnh build, test, run, clean nhanh
├── run_salehunter_bot.bat      # Script khởi động 1-click trên Windows
├── .env.example                # File mẫu cấu hình bot và database
├── .gitignore                  # Bỏ qua build artifact và database file
└── src/
    ├── main.rs                 # Điểm khởi chạy ứng dụng & kiểm thử tích hợp
    ├── config.rs               # Quản lý nạp cấu hình hệ thống từ môi trường
    ├── utils.rs                # Tiện ích tiền tệ VND, mã hóa, sinh mã giới thiệu
    ├── bot/
    │   ├── mod.rs              # Khởi tạo Dispatcher và vòng lặp Bot
    │   ├── handlers.rs         # Bộ xử lý Commands, Message và Callback Query
    │   └── keyboards.rs        # Giao diện bàn phím Inline Menu trực quan
    ├── db/
    │   ├── mod.rs              # Khởi tạo LibSQL client và schema migration
    │   ├── models.rs           # Khai báo Struct User, Click, Order, Withdrawal
    │   └── repository.rs       # Tầng truy vấn dữ liệu chuẩn bị (Prepared Queries)
    ├── web/
    │   └── mod.rs              # Web Portal `/`, đổi link `/convert`, cầu nối Mobile & Admin Dashboard
    └── services/
        ├── mod.rs
        ├── accesstrade.rs      # Kết nối ACCESSTRADE API (chiến dịch, kho mã, đơn hàng)
        ├── shopee_affiliate.rs # Ký Open API Shopee & xây dựng link an_redir / social params
        ├── order_tracker.rs    # Đối soát đơn tự động mỗi 10 phút & báo Telegram Admin
        ├── deal_parser.rs      # Bóc tách nhiều link, giải mã shortlink & tạo affiliate URL
        └── cashback_engine.rs  # Bộ máy phân chia hoa hồng & xử lý hoàn tiền
```

---

## 🚀 Hướng Dẫn Cài Đặt & Chạy Bot

### Bước 1: Chuẩn Bị Token Telegram
1. Mở Telegram, tìm bot `@BotFather`.
2. Gõ `/newbot` và làm theo hướng dẫn để đặt tên và lấy `API Token`.
3. Lấy username của bot bạn vừa tạo (ví dụ: `MySaleHunterBot`).

### Bước 2: Thiết Lập Cấu Hình Môi Trường
Sao chép file `.env.example` thành file `.env`:
```bash
copy .env.example .env
```
Mở file `.env` và điền các thông tin:
```ini
TELEGRAM_BOT_TOKEN=123456789:ABCdefGhIJKlmNoPQRstuVWXyz
BOT_USERNAME=MySaleHunterBot
DATABASE_PATH=salehunter.db
CASHBACK_RATE=70.0
REFERRAL_BONUS_RATE=10.0
MIN_WITHDRAWAL_VND=50000
ACCESSTRADE_DEEPLINK_TEMPLATE=https://fast.accesstrade.com.vn/deep_link/630000000000?url={CLEAN_URL}&sub_id1={USER_ID}&sub_id2={REF_ID}&sub_id3={CLICK_ID}
```

### Bước 3: Chạy Bot
- **Chạy trên Render (24/7 tự động Webhook + Keep-Alive):**
  - Hệ thống tự động nhận diện `RENDER_EXTERNAL_URL` để bật **Telegram Webhook** tại `/telegram/webhook` (xác thực header `X-Telegram-Bot-Api-Secret-Token`) kết hợp vòng lặp self-ping `/health` mỗi 9 phút và workflow GitHub Actions tại `.github/workflows/keep-alive.yml` để giữ container hoạt động liên tục 24/7.
- **Chạy trên máy cá nhân (Long Polling):**
  - **Cách 1:** Nhấp đúp vào file `run_salehunter_bot.bat` trên Windows.
  - **Cách 2:** Chạy lệnh `cargo run --release` hoặc `make run`.

---

## 📱 Danh Sách Các Lệnh Telegram Hiện Tại

| Lệnh | Ý nghĩa |
| :--- | :--- |
| `/start` | Bắt đầu sử dụng bot, bật menu nhanh và hướng dẫn dán link |
| `/help` | Xem hướng dẫn 3 bước nhận link ưu đãi & mẹo áp mã |
| `/share` | Lấy link chia sẻ bot cho bạn bè cùng săn sale |
| `/stats` | Xem thống kê người dùng, lượt dán link, đơn đối soát và lấy **Telegram ID** |

*(Ngoài ra người dùng chỉ cần **dán trực tiếp 1 hoặc tối đa 5 link sản phẩm — kể cả bài có ảnh/video kèm caption —** từ Shopee/Lazada/Tiki/TikTok Shop/Alibaba vào khung chat, Bot sẽ tự động phân tích, bọc link 8 ký tự và gắn mã khớp nếu có).*

---

## 🧪 Kiểm Thử (Unit Tests)

Chạy bộ kiểm thử tự động của Rust:
```bash
cargo test
```
Kiểm tra toàn bộ luồng nghiệp vụ: phân tích URL, bóc tách link sàn, làm sạch tracking, tính toán hoa hồng, cấu hình fallback, xác thực Telegram Webhook và xây dựng Web Admin Router.

---

## 🔒 Quy Ước Bảo Mật
- Không lưu bất kỳ thông tin đăng nhập hoặc dữ liệu nhạy cảm vào mã nguồn.
- CSDL LibSQL lưu trữ an toàn trong thư mục dự án trên ổ đĩa dữ liệu (ổ `F:`).
- Lọc sạch toàn bộ ký tự điều khiển (CRLF) trước khi đưa vào hệ thống ghi log.

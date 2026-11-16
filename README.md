# 🚀 DealX - Bot Săn Mã Giảm Giá & Web Quản Trị Đa Sàn (Rust + LibSQL)

Hệ thống Bot Telegram săn sale đa sàn (**DealX - Trợ Lý Săn Mã & Ép Giá**) tích hợp **Web Admin Dashboard Quản Trị Tối Mật**, phân tích link sản phẩm đa sàn (Shopee, Lazada, Tiki, TikTok Shop) và tự động gắn mã giảm giá sâu 20% - 50%, voucher Video, Freeship, được phát triển 100% bằng **ngôn ngữ Rust** kết hợp cơ sở dữ liệu **LibSQL** (Turso) và framework web **Axum**.

---

## 🌟 Tính Năng Nổi Bật

### 1. Web Admin Dashboard Quản Trị Độc Quyền (Bảo Mật Tối Cao)
- **Chỉ DUY NHẤT 1 tài khoản Admin:** Không cho phép bất kỳ ai đăng ký mới, chặn 100% truy cập trái phép.
- **Bảo mật chuẩn:** Xác thực Cookie HTTP-only, chống Timing Attack với `subtle::ConstantTimeEq`.
- **Thống kê thời gian thực:** Xem tổng số người dùng bot, tổng số link đã chuyển đổi, số lượt click theo từng sàn (Shopee, Lazada, TikTok, Tiki).
- **Quản lý kho Mã Độc Quyền & Đồng Bộ Kho Mã 20% - 35%+:** Nhập lẻ hoặc dán hàng loạt mã độc quyền hợp tác từ AM/MCN/Shop (hỗ trợ cả dấu `|` hoặc copy từ Excel/Google Sheets), bật/tắt hoặc xoá mã; tự động quét kho mã 20%-35%+ thật từ ACCESSTRADE API qua lệnh `/deals`.
- **Xem nhật ký dán link:** Theo dõi 50 link khách hàng vừa gửi vào bot gần nhất kèm link affiliate tương ứng.

### 2. Phân Tích & Làm Sạch Link Sản Phẩm Tự Động
- **Đa sàn TMĐT:** Hỗ trợ Shopee (`shopee.vn`, `shope.ee`), Lazada (`lazada.vn`, `s.lazada.vn`), Tiki (`tiki.vn`), TikTok Shop.
- **Giải mã shortlink:** Tự động resolve các đường link rút gọn dạng `shope.ee/...` hay `s.lazada.vn/...` để lấy URL sản phẩm gốc.
- **Xóa tracking rác (Clean URL):** Loại bỏ toàn bộ tham số rác như `utm_source`, `utm_medium`, `spm`, `ex_tracking`,... để đưa về link chuẩn nhất (`/product/{shopid}/{itemid}`).

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
    └── services/
        ├── mod.rs
        ├── deal_parser.rs      # Bóc tách, giải mã shortlink & tạo affiliate URL
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

## 📱 Danh Sách Các Lệnh Người Dùng

| Lệnh | Ý nghĩa |
| :--- | :--- |
| `/start [ref_code]` | Bắt đầu sử dụng bot, liên kết mã giới thiệu bạn bè và mở Menu chính |
| `/help` | Xem hướng dẫn quy trình săn sale & nhận hoàn tiền 3 bước |
| `/wallet` hoặc `/vi` | Xem số dư khả dụng, số dư chờ duyệt, hoa hồng F1 và mã giới thiệu |
| `/deals [sàn]` | Xem các mã giảm giá và voucher hot hôm nay (Shopee, Lazada, Tiki) |
| `/share` | Lấy link chia sẻ giới thiệu bạn bè nhận hoa hồng 10% trọn đời |
| `/history` | Xem 5 đơn hoàn tiền gần nhất và 5 lượt tạo link |
| `/withdraw <tiền> <ngân_hàng> <stk> <tên>` | Tạo yêu cầu rút tiền về tài khoản ngân hàng |

*(Ngoài ra người dùng chỉ cần **dán trực tiếp link sản phẩm Shopee/Lazada/Tiki/TikTok Shop** vào khung chat, Bot sẽ tự động phân tích và tạo nút mua hoàn tiền).*

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

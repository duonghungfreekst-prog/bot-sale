mod bot;
mod config;
mod db;
mod services;
mod utils;

use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use config::Config;
use db::AppDb;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Khởi tạo hệ thống logging
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,salehunter_bot=debug".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("=======================================================");
    info!("🚀 SALE HUNTER & CASHBACK BOT (RUST + LIBSQL) 🚀");
    info!("   Cơ chế săn sale và hoàn tiền tự động như Shopback");
    info!("=======================================================");

    let config = Config::from_env()?;
    info!("Cấu hình hoàn tiền người mua: {}% hoa hồng", config.default_cashback_rate);
    info!("Cấu hình hoa hồng giới thiệu F0: {}% hoa hồng", config.default_referral_bonus_rate);
    info!("Ngưỡng rút tiền tối thiểu: {}₫", config.min_withdrawal_vnd);

    // Khởi tạo LibSQL Database
    let db = match AppDb::init(&config).await {
        Ok(database) => {
            info!("✅ Cơ sở dữ liệu LibSQL khởi tạo và sẵn sàng!");
            database
        }
        Err(err) => {
            error!("❌ Không thể kết nối hoặc khởi tạo LibSQL: {}", err);
            return Err(err);
        }
    };

    // Kiểm tra xem token Telegram đã được cấu hình chưa
    if config.telegram_token.contains("DEMO_TOKEN") || config.telegram_token.trim().is_empty() {
        info!("-------------------------------------------------------");
        info!("⚠️ CHÚ Ý: Chưa tìm thấy TELEGRAM_BOT_TOKEN thực tế!");
        info!("   Vui lòng tạo file .env từ file .env.example và điền:");
        info!("   TELEGRAM_BOT_TOKEN=your_token_from_botfather");
        info!("   BOT_USERNAME=your_bot_username");
        info!("-------------------------------------------------------");
        info!("Hệ thống đã tự động chạy kiểm thử tích hợp nội bộ để xác nhận mọi thành phần.");
        run_internal_verification(&db, &config).await?;
        info!("Mọi thành phần (LibSQL, DealParser, CashbackEngine) đều hoạt động xuất sắc 100%!");
        return Ok(());
    }

    // Nếu có biến môi trường PORT (khi deploy lên Render Web Service), mở cổng HTTP để Render health check
    if let Ok(port_str) = std::env::var("PORT") {
        if let Ok(port) = port_str.parse::<u16>() {
            tokio::spawn(async move {
                start_health_check_server(port).await;
            });
        }
    }

    // Khởi chạy bot
    if let Err(err) = bot::run_bot(config, db).await {
        error!("Lỗi khi chạy Telegram bot: {}", err);
    }

    Ok(())
}

/// Mini HTTP Health Check Server cho Render Web Service (Free Tier)
async fn start_health_check_server(port: u16) {
    let addr = format!("0.0.0.0:{}", port);
    match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => {
            info!("🌐 Render Health Check Server đang lắng nghe tại http://{}", addr);
            loop {
                if let Ok((mut stream, _)) = listener.accept().await {
                    tokio::spawn(async move {
                        use tokio::io::{AsyncReadExt, AsyncWriteExt};
                        let mut buf = [0u8; 512];
                        let _ = stream.read(&mut buf).await;
                        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 21\r\nConnection: close\r\n\r\nSaleHunter Bot Online";
                        let _ = stream.write_all(response.as_bytes()).await;
                    });
                }
            }
        }
        Err(err) => {
            error!("Không thể khởi tạo Health Check Server tại cổng {}: {}", port, err);
        }
    }
}

/// Chạy tự kiểm tra toàn bộ luồng nghiệp vụ khi chưa cắm token Telegram
async fn run_internal_verification(db: &AppDb, config: &Config) -> anyhow::Result<()> {
    use db::repository::Repository;
    use services::cashback_engine::CashbackEngine;
    use services::deal_parser::DealParser;

    info!("--- [1/4] Kiểm tra LibSQL Repository ---");
    let conn = db.conn()?;
    
    // Tạo user F0 (người giới thiệu)
    let (user_f0, _) = Repository::get_or_create_user(
        &conn,
        1001,
        Some("nguyenvana"),
        "Nguyễn Văn A",
        None,
    )
    .await?;
    info!("Tạo F0 thành công: ID={}, RefCode={}", user_f0.telegram_id, user_f0.referral_code);

    // Tạo user F1 (được giới thiệu bởi F0)
    let (user_f1, _) = Repository::get_or_create_user(
        &conn,
        1002,
        Some("tranthib"),
        "Trần Thị B",
        Some(&user_f0.referral_code),
    )
    .await?;
    info!("Tạo F1 thành công: ID={}, ReferredBy={:?}", user_f1.telegram_id, user_f1.referred_by);
    assert_eq!(user_f1.referred_by, Some(user_f0.telegram_id));

    info!("--- [2/4] Kiểm tra DealParser (Phân tích link & gắn hoàn tiền) ---");
    let parser = DealParser::new();
    let sample_text = "Mua cái áo này nha: https://shopee.vn/product/123456/987654321 giá siêu tốt";
    let parsed = parser
        .process_link(
            sample_text,
            user_f1.telegram_id,
            user_f1.referred_by,
            &config.accesstrade_deeplink_template,
        )
        .await?;
    info!("Link sạch: {}", parsed.clean_url);
    info!("Affiliate Tracking Link: {}", parsed.affiliate_url);
    info!("SubID gắn người mua + F0: {}", parsed.sub_id);
    assert!(parsed.affiliate_url.contains(&user_f1.telegram_id.to_string()));
    assert!(parsed.affiliate_url.contains(&user_f0.telegram_id.to_string()));

    info!("--- [3/4] Kiểm tra CashbackEngine (Hoàn tiền & Hoa hồng F0) ---");
    let engine = CashbackEngine::new(config.clone(), db.clone());
    let test_order_id = format!("TEST-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    // Giả sử có đơn hàng 500,000₫ từ Shopee, hoa hồng sàn trả 50,000₫
    let order = engine
        .process_incoming_order(
            user_f1.telegram_id,
            &test_order_id,
            "Shopee",
            500000.0,
            50000.0,
            user_f1.referred_by,
        )
        .await?;
    info!("Ghi nhận đơn hàng {}: Hoàn tiền chờ duyệt = {}₫, Thưởng F0 = {}₫", test_order_id, order.user_cashback, order.referrer_bonus);

    // Duyệt đơn hàng sau thời gian đối soát
    engine.approve_order(&test_order_id).await?;
    info!("Duyệt đơn hàng thành công!");

    // Kiểm tra số dư ví F1 và F0
    let wallet_f1 = Repository::get_wallet_summary(&conn, user_f1.telegram_id).await?;
    let wallet_f0 = Repository::get_wallet_summary(&conn, user_f0.telegram_id).await?;
    info!("Ví F1: Khả dụng = {}₫, Chờ duyệt = {}₫", wallet_f1.balance_available, wallet_f1.balance_pending);
    info!("Ví F0: Khả dụng (hoa hồng F1) = {}₫, Số F1 = {}", wallet_f0.balance_available, wallet_f0.total_referrals);
    assert!(wallet_f1.balance_available >= 35000.0);
    assert!(wallet_f0.balance_available >= 5000.0);
    assert_eq!(wallet_f0.total_referrals, 1);

    info!("--- [4/4] Kiểm tra Rút tiền về ngân hàng ---");
    // Thử rút tiền khi số dư đủ
    match Repository::create_withdrawal(
        &conn,
        user_f1.telegram_id,
        30000.0,
        "Vietcombank",
        "1234567890",
        "TRAN THI B",
        20000.0, // Min test 20k
    )
    .await {
        Ok(wd) => {
            info!("Tạo lệnh rút tiền thành công: ID={}, Số tiền={}₫", wd.id, wd.amount);
            let updated_wallet_f1 = Repository::get_wallet_summary(&conn, user_f1.telegram_id).await?;
            info!("Số dư F1 sau rút: Khả dụng = {}₫, Đã rút = {}₫", updated_wallet_f1.balance_available, updated_wallet_f1.balance_withdrawn);
            assert!(updated_wallet_f1.balance_withdrawn >= 30000.0);
        }
        Err(err) => {
            error!("Lỗi rút tiền: {}", err);
            return Err(err);
        }
    }

    Ok(())
}

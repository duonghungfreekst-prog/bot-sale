mod bot;
mod config;
mod db;
mod services;
mod utils;
mod web;

use std::sync::Arc;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use config::Config;
use db::repository::Repository;
use db::AppDb;
use services::accesstrade::AccessTradeClient;

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

    // Khởi tạo Client ACCESSTRADE API dùng chung cho cả Web Admin & Telegram Bot
    let accesstrade = Arc::new(AccessTradeClient::new());

    if config.turso_url.is_none() || config.turso_auth_token.is_none() {
        warn!("⚠️ Đang dùng SQLite cục bộ ({}): dữ liệu DB sẽ bị reset khi container khởi động lại. Khuyên dùng Turso Cloud (TURSO_DATABASE_URL) để lưu vĩnh viễn.", config.database_path);
    }

    // Tự động nạp trước và cập nhật ĐỊNH KỲ MỖI PHÚT (60s) chiến dịch & kho mã toàn thị trường ACCESSTRADE
    {
        let warmup_db = db.clone();
        let warmup_at = accesstrade.clone();
        let env_token = config.accesstrade_api_token.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(60));
            let mut cycle: u64 = 0;
            loop {
                ticker.tick().await;
                let db_token = match warmup_db.conn() {
                    Ok(conn) => Repository::get_setting(&conn, "accesstrade_api_token")
                        .await
                        .ok()
                        .flatten()
                        .filter(|s| !s.trim().is_empty()),
                    Err(_) => None,
                };
                if let Some(ref tok) = db_token.or_else(|| env_token.clone()) {
                    let is_full = cycle % 5 == 0;
                    if is_full {
                        if let Err(err) = warmup_at.get_approved_campaigns(tok, true).await {
                            warn!("Không thể đồng bộ sẵn ACCESSTRADE khi khởi động/định kỳ: {}", err);
                        }
                    }
                    if let Err(err) = warmup_at.sync_market_vouchers_tick(tok, is_full).await {
                        warn!("Không thể cập nhật kho mã mỗi phút ACCESSTRADE: {}", err);
                    }
                } else {
                    warn!("⚠️ Chưa tìm thấy ACCESSTRADE_API_TOKEN trong Environment hoặc DB.");
                }
                cycle = cycle.wrapping_add(1);
            }
        });
    }

    // 1. Luôn khởi chạy Web Admin Dashboard & Health Check Server trước tiên
    let (webhook_tx, webhook_rx) = if config.public_base_url.is_some() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();

    let web_db = db.clone();
    let web_config = config.clone();
    let web_at = accesstrade.clone();
    let web_port = web_config.port;
    let web_handle = tokio::spawn(async move {
        if let Err(err) = web::start_web_server(
            web_config.port,
            web_db,
            web_config,
            web_at,
            webhook_tx,
            Some(ready_tx),
        )
        .await
        {
            error!("Lỗi khi khởi chạy Web Admin Server: {}", err);
        }
    });

    info!("🌐 Web Admin Dashboard & Health Check đang lắng nghe tại cổng {}", web_port);

    // 1.5 Nếu chạy trên Render/Cloud có public_base_url: bật self-ping giữ ấm mỗi 9 phút
    if let Some(base_url) = config.public_base_url.clone() {
        spawn_keepalive_task(base_url);
    }

    // 2. Kiểm tra xem token Telegram đã được cấu hình chưa
    if config.telegram_token.contains("DEMO_TOKEN") || config.telegram_token.trim().is_empty() {
        info!("-------------------------------------------------------");
        info!("⚠️ CHÚ Ý: Chưa tìm thấy TELEGRAM_BOT_TOKEN trong biến môi trường!");
        info!("   Web Admin Dashboard vẫn đang chạy phục vụ tại cổng {}", web_port);
        info!("   Vui lòng vào tab 'Environment' trên Render để thêm:");
        info!("   TELEGRAM_BOT_TOKEN=<TELEGRAM_BOT_TOKEN_CUA_BAN>");
        info!("   BOT_USERNAME=vvipass_vip_bot");
        info!("-------------------------------------------------------");
        
        // Giữ Web Admin Server chạy liên tục, tuyệt đối không exit làm sập Render Web Service
        let _ = web_handle.await;
        return Ok(());
    }

    // 3. Khởi chạy Telegram bot song song cùng Web Admin Server
    tokio::select! {
        _ = web_handle => {
            error!("Web Server đã dừng.");
        }
        res = bot::run_bot(config, db, accesstrade, webhook_rx, Some(ready_rx)) => {
            if let Err(err) = res {
                error!("Lỗi khi chạy Telegram bot: {}", err);
            }
        }
    }

    Ok(())
}

/// Gửi HTTP GET định kỳ tới /health mỗi 9 phút để giữ container Cloud luôn sẵn sàng
fn spawn_keepalive_task(base_url: String) {
    let health_url = format!("{}/health", base_url);
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("DealX-KeepAlive/1.0")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        tokio::time::sleep(std::time::Duration::from_secs(45)).await;
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(540));
        loop {
            ticker.tick().await;
            match client.get(&health_url).send().await {
                Ok(resp) => {
                    info!("[KEEPALIVE] Ping {} -> HTTP {}", health_url, resp.status());
                }
                Err(err) => {
                    warn!("[KEEPALIVE] Không thể ping {}: {}", health_url, err);
                }
            }
        }
    });
}

/// Chạy tự kiểm tra toàn bộ luồng nghiệp vụ khi chưa cắm token Telegram
#[allow(dead_code)]
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
    let aff_settings = services::deal_parser::AffiliateSettings {
        deeplink_template: &config.accesstrade_deeplink_template,
        accesstrade_token: config.accesstrade_api_token.as_deref(),
        campaign_shopee: None,
        campaign_lazada: None,
        campaign_tiki: None,
        campaign_tiktok: None,
        shopee_affiliate_id: config.shopee_affiliate_id.as_deref(),
        shopee_app_id: config.shopee_app_id.as_deref(),
        shopee_app_secret: config.shopee_app_secret.as_deref(),
    };
    let parsed = parser
        .process_link(
            sample_text,
            user_f1.telegram_id,
            user_f1.referred_by,
            aff_settings,
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

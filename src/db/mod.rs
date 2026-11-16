pub mod models;
pub mod repository;

use std::sync::Arc;
use anyhow::{Context, Result};
use libsql::Builder;
use tracing::info;

use crate::config::Config;

#[derive(Clone)]
pub struct AppDb {
    pub db: Arc<libsql::Database>,
}

impl AppDb {
    pub async fn init(config: &Config) -> Result<Self> {
        let db = if let (Some(url), Some(token)) = (&config.turso_url, &config.turso_auth_token) {
            info!("Kết nối đến Turso Database từ xa: {}", url);
            Builder::new_remote(url.clone(), token.clone())
                .build()
                .await
                .context("Không thể khởi tạo kết nối Turso từ xa")?
        } else {
            info!("Khởi tạo LibSQL Database cục bộ tại: {}", config.database_path);
            Builder::new_local(&config.database_path)
                .build()
                .await
                .context("Không thể khởi tạo LibSQL Database cục bộ")?
        };

        let app_db = Self {
            db: Arc::new(db),
        };

        app_db.migrate().await?;
        app_db.seed_deals().await?;

        Ok(app_db)
    }

    pub fn conn(&self) -> Result<libsql::Connection> {
        self.db.connect().context("Không thể mở kết nối LibSQL")
    }

    async fn migrate(&self) -> Result<()> {
        let conn = self.conn()?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS users (
                telegram_id INTEGER PRIMARY KEY,
                username TEXT,
                full_name TEXT NOT NULL,
                referral_code TEXT UNIQUE NOT NULL,
                referred_by INTEGER,
                balance_available REAL NOT NULL DEFAULT 0.0,
                balance_pending REAL NOT NULL DEFAULT 0.0,
                balance_withdrawn REAL NOT NULL DEFAULT 0.0,
                created_at TEXT NOT NULL
            );",
            (),
        )
        .await
        .context("Tạo bảng users thất bại")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS clicks (
                id TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL,
                platform TEXT NOT NULL,
                original_url TEXT NOT NULL,
                clean_url TEXT NOT NULL,
                affiliate_url TEXT NOT NULL,
                sub_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(telegram_id)
            );",
            (),
        )
        .await
        .context("Tạo bảng clicks thất bại")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS cashback_orders (
                id TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL,
                order_id TEXT NOT NULL,
                platform TEXT NOT NULL,
                order_amount REAL NOT NULL,
                total_commission REAL NOT NULL,
                user_cashback REAL NOT NULL,
                referrer_bonus REAL NOT NULL,
                referrer_id INTEGER,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(telegram_id)
            );",
            (),
        )
        .await
        .context("Tạo bảng cashback_orders thất bại")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS withdrawals (
                id TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL,
                amount REAL NOT NULL,
                bank_name TEXT NOT NULL,
                account_number TEXT NOT NULL,
                account_name TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(telegram_id)
            );",
            (),
        )
        .await
        .context("Tạo bảng withdrawals thất bại")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS deals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                platform TEXT NOT NULL,
                title TEXT NOT NULL,
                coupon_code TEXT,
                discount_desc TEXT NOT NULL,
                min_spend REAL NOT NULL DEFAULT 0.0,
                deal_url TEXT NOT NULL,
                expire_at TEXT,
                is_active INTEGER NOT NULL DEFAULT 1
            );",
            (),
        )
        .await
        .context("Tạo bảng deals thất bại")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
            (),
        )
        .await
        .context("Tạo bảng settings thất bại")?;

        info!("LibSQL schema migration thành công.");
        Ok(())
    }

    async fn seed_deals(&self) -> Result<()> {
        let conn = self.conn()?;
        let mut rows = conn.query("SELECT COUNT(*) FROM deals", ()).await?;
        let count: i64 = if let Some(row) = rows.next().await? {
            row.get(0)?
        } else {
            0
        };

        if count == 0 {
            info!("Nạp danh sách mã giảm giá và deals ban đầu...");
            let initial_deals = [
                ("Shopee", "Mã Giảm 50K Toàn Sàn", Some("SHOPEE50K"), "Giảm 50,000₫ cho đơn từ 250k", 250000.0, "https://shopee.vn/m/ma-giam-gia"),
                ("Shopee", "Freeship Xtra Giảm 30K", Some("FREESHIP30"), "Miễn phí vận chuyển tới 30,000₫", 99000.0, "https://shopee.vn/m/mien-phi-van-chuyen"),
                ("Lazada", "Voucher Tích Lũy 100K", Some("LAZADA100"), "Giảm 10k mỗi 150k, tối đa 100k", 150000.0, "https://www.lazada.vn/voucher"),
                ("Lazada", "Hoàn Tiền Max 12%", None, "Tự động hoàn tối đa 50,000 xu", 0.0, "https://www.lazada.vn/hoan-tien-max"),
                ("Tiki", "TikiNOW Giảm 20K Giao 2H", Some("TIKINOW20"), "Giảm 20,000₫ phí giao siêu tốc", 100000.0, "https://tiki.vn/khuyen-mai/ma-giam-gia"),
                ("TikTokShop", "Flash Sale Trợ Giá 30%", Some("TTSALE30"), "Giảm trực tiếp 30k trên live", 120000.0, "https://www.tiktok.com"),
            ];

            for (platform, title, code, desc, min_spend, url) in initial_deals {
                conn.execute(
                    "INSERT INTO deals (platform, title, coupon_code, discount_desc, min_spend, deal_url, is_active)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
                    libsql::params![platform, title, code, desc, min_spend, url],
                )
                .await?;
            }
            info!("Đã nạp xong 6 deals khuyến mãi mẫu.");
        }

        Ok(())
    }
}

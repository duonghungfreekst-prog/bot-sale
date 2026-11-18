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
                is_active INTEGER NOT NULL DEFAULT 1,
                is_exclusive INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT ''
            );",
            (),
        )
        .await
        .context("Tạo bảng deals thất bại")?;

        // Bổ sung cột mới an toàn cho DB cũ (SQLite bỏ qua lỗi nếu cột đã tồn tại)
        let _ = conn
            .execute(
                "ALTER TABLE deals ADD COLUMN is_exclusive INTEGER NOT NULL DEFAULT 0",
                (),
            )
            .await;
        let _ = conn
            .execute(
                "ALTER TABLE deals ADD COLUMN created_at TEXT NOT NULL DEFAULT ''",
                (),
            )
            .await;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
            (),
        )
        .await
        .context("Tạo bảng settings thất bại")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS short_links (
                code TEXT PRIMARY KEY,
                target_url TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                platform TEXT NOT NULL DEFAULT '',
                clicks_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );",
            (),
        )
        .await
        .context("Tạo bảng short_links thất bại")?;

        let _ = conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_short_links_target ON short_links(target_url);",
                (),
            )
            .await;

        info!("LibSQL schema migration thành công.");
        Ok(())
    }
}

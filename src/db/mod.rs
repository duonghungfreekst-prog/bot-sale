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
        let turso_creds = config
            .turso_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .zip(
                config
                    .turso_auth_token
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
            );

        let db = if let Some((url, token)) = turso_creds {
            info!("☁️ Kết nối đến Turso Cloud Database từ xa: {}", url);
            Builder::new_remote(url.to_string(), token.to_string())
                .build()
                .await
                .context("Không thể khởi tạo kết nối Turso Cloud từ xa")?
        } else if config.is_cloud_runtime() {
            anyhow::bail!(
                "Đã tắt fallback SQLite cục bộ trên môi trường Hosting/Cloud: bắt buộc phải cấu hình TURSO_DATABASE_URL và TURSO_AUTH_TOKEN (từ chối tạo file `{}`).",
                config.database_path
            );
        } else {
            info!(
                "💾 Khởi tạo LibSQL Database cục bộ (PC Local) tại: {}",
                config.database_path
            );
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

    /// Hàm chỉ phục vụ `cargo test`: khởi tạo DB cục bộ/in-memory độc lập để kiểm thử Web Admin Cloud không cần mạng Turso.
    #[cfg(test)]
    pub async fn init_local_for_test(database_path: &str) -> Result<Self> {
        let db = Builder::new_local(database_path)
            .build()
            .await
            .context("Không thể khởi tạo LibSQL Database cục bộ cho test")?;
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

        conn.execute(
            "CREATE TABLE IF NOT EXISTS admin_sessions (
                token_hash TEXT PRIMARY KEY,
                expires_at INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );",
            (),
        )
        .await
        .context("Tạo bảng admin_sessions thất bại")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS tracked_orders (
                order_key TEXT PRIMARY KEY,
                platform TEXT NOT NULL,
                order_id TEXT NOT NULL,
                status TEXT NOT NULL,
                order_amount REAL NOT NULL DEFAULT 0.0,
                commission REAL NOT NULL DEFAULT 0.0,
                sub_id TEXT NOT NULL DEFAULT '',
                order_time TEXT NOT NULL DEFAULT '',
                notified_at TEXT NOT NULL
            );",
            (),
        )
        .await
        .context("Tạo bảng tracked_orders thất bại")?;

        let _ = conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_tracked_orders_notified ON tracked_orders(notified_at DESC);",
                (),
            )
            .await;

        let _ = conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_clicks_created_at ON clicks(created_at DESC);",
                (),
            )
            .await;
        let _ = conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_clicks_platform ON clicks(platform);",
                (),
            )
            .await;
        let _ = conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_deals_active_exclusive ON deals(is_active, is_exclusive DESC, id DESC);",
                (),
            )
            .await;
        let _ = conn
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_admin_sessions_expires ON admin_sessions(expires_at);",
                (),
            )
            .await;

        // Dọn dẹp triệt để các bản ghi seed giả mẫu cũ (AUTO_*, SHOPEE50K...) và các bài hướng dẫn rác quét nhầm từ trước
        let _ = conn
            .execute(
                "DELETE FROM deals WHERE
                    (is_exclusive = 0 AND (
                        coupon_code LIKE 'AUTO_%'
                        OR coupon_code IN ('SHOPEE50K', 'FREESHIP30', 'LAZADA100', 'TIKINOW20', 'TTSALE30')
                        OR (platform = 'Lazada' AND coupon_code IS NULL AND title = 'Hoàn Tiền Max 12%')
                    ))
                    OR (
                        (coupon_code LIKE 'KOL_%' OR coupon_code LIKE 'SHP_AUTO_%')
                        AND (
                            LOWER(discount_desc) LIKE '%hướng dẫn lấy mã%'
                            OR LOWER(discount_desc) LIKE '%hướng dẫn săn mã%'
                            OR LOWER(discount_desc) LIKE '%hướng dẫn áp mã%'
                            OR LOWER(discount_desc) LIKE '%khi ấn link lấy mã%'
                            OR LOWER(discount_desc) LIKE '%cách lấy mã%'
                        )
                    );",
                (),
            )
            .await;

        info!("LibSQL schema migration thành công.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cloud_config(db_path: &str, turso_url: Option<&str>, turso_token: Option<&str>) -> Config {
        Config {
            telegram_token: "test".to_string(),
            bot_username: "vvipass_vip_bot".to_string(),
            database_path: db_path.to_string(),
            turso_url: turso_url.map(str::to_string),
            turso_auth_token: turso_token.map(str::to_string),
            default_cashback_rate: 70.0,
            default_referral_bonus_rate: 10.0,
            min_withdrawal_vnd: 50000.0,
            accesstrade_deeplink_template: String::new(),
            accesstrade_api_token: None,
            shopee_affiliate_id: None,
            shopee_app_id: None,
            shopee_app_secret: None,
            shopee_social_params: None,
            admin_username: "admin".to_string(),
            admin_password: "StrongPassword!123".to_string(),
            admin_telegram_id: None,
            port: 10000,
            public_base_url: Some("https://hosting.example".to_string()),
            shortlink_base_url: None,
            telegram_webhook_secret: "secret_xyz".to_string(),
        }
    }

    #[tokio::test]
    async fn test_cloud_mode_rejects_local_sqlite_fallback() {
        let forbidden_path = format!(
            "target/must_not_create_cloud_{}.db",
            uuid::Uuid::new_v4().simple()
        );
        let cfg_missing_both = sample_cloud_config(&forbidden_path, None, None);
        let err_both = AppDb::init(&cfg_missing_both)
            .await
            .err()
            .expect("Cloud mode without Turso must fail instead of falling back to SQLite");
        assert!(err_both.to_string().contains("TURSO_DATABASE_URL"));
        assert!(!std::path::Path::new(&forbidden_path).exists());

        let cfg_missing_token = sample_cloud_config(
            &forbidden_path,
            Some("libsql://example.turso.io"),
            Some("   "),
        );
        assert!(AppDb::init(&cfg_missing_token).await.is_err());
        assert!(!std::path::Path::new(&forbidden_path).exists());

        // Chế độ PC Local (public_base_url = None) vẫn khởi tạo được SQLite cục bộ bình thường
        let local_path = format!("target/test_pc_local_ok_{}.db", uuid::Uuid::new_v4().simple());
        let mut cfg_local = sample_cloud_config(&local_path, None, None);
        cfg_local.public_base_url = None;
        if !cfg_local.is_cloud_runtime() {
            let db_ok = AppDb::init(&cfg_local).await;
            assert!(db_ok.is_ok());
        }
        let _ = std::fs::remove_file(&local_path);
    }
}

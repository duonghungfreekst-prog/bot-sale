pub mod models;
pub mod repository;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::{Context, Result};
use chrono::Utc;
use libsql::Builder;
use tokio::sync::RwLock;
use tracing::info;

use crate::config::Config;
use models::{DealItem, ShortLinkRecord};
use repository::Repository;

const L1_SETTINGS_TTL: Duration = Duration::from_secs(20);
const L1_DEALS_TTL: Duration = Duration::from_secs(20);
const SHORTLINK_CACHE_MAX: usize = 4096;

struct DbRuntimeCache {
    enabled: AtomicBool,
    settings: RwLock<Option<(HashMap<String, String>, Instant)>>,
    active_deals: RwLock<Option<(Vec<DealItem>, Instant)>>,
    short_code_by_url: RwLock<HashMap<String, String>>,
    short_link_by_code: RwLock<HashMap<String, ShortLinkRecord>>,
}

impl DbRuntimeCache {
    fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            settings: RwLock::new(None),
            active_deals: RwLock::new(None),
            short_code_by_url: RwLock::new(HashMap::new()),
            short_link_by_code: RwLock::new(HashMap::new()),
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

fn prune_bounded_map<K: Clone + Eq + std::hash::Hash, V>(map: &mut HashMap<K, V>, max_entries: usize) {
    if map.len() < max_entries {
        return;
    }
    let drop_n = (max_entries / 5).max(1);
    let keys: Vec<K> = map.keys().take(drop_n).cloned().collect();
    for k in keys {
        map.remove(&k);
    }
}

#[derive(Clone)]
pub struct AppDb {
    pub db: Arc<libsql::Database>,
    cache: Arc<DbRuntimeCache>,
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
            cache: Arc::new(DbRuntimeCache::new(!cfg!(test))),
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
            cache: Arc::new(DbRuntimeCache::new(false)),
        };
        app_db.migrate().await?;
        Ok(app_db)
    }

    #[cfg(test)]
    pub fn set_cache_enabled_for_test(&self, enabled: bool) {
        self.cache.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn conn(&self) -> Result<libsql::Connection> {
        self.db.connect().context("Không thể mở kết nối LibSQL")
    }

    /// Đọc toàn bộ cấu hình nhẹ từ L1 RAM Cache (TTL 20s) hoặc chỉ 1 câu SQL `get_settings_map` khi cache miss.
    pub async fn get_settings_map_cached(&self) -> Result<HashMap<String, String>> {
        if self.cache.is_enabled() {
            let guard = self.cache.settings.read().await;
            if let Some((ref map, ts)) = *guard
                && ts.elapsed() < L1_SETTINGS_TTL
            {
                return Ok(map.clone());
            }
        }

        let conn = self.conn()?;
        let fresh = Repository::get_settings_map(&conn).await?;
        if self.cache.is_enabled() {
            let mut guard = self.cache.settings.write().await;
            *guard = Some((fresh.clone(), Instant::now()));
        }
        Ok(fresh)
    }

    /// Xóa L1 cache `settings` ngay lập tức sau khi Admin hoặc Harvester cập nhật cấu hình.
    pub async fn invalidate_settings_cache(&self) {
        let mut guard = self.cache.settings.write().await;
        *guard = None;
    }

    /// Đọc danh sách deal đang hoạt động từ L1 RAM Cache (TTL 20s), có thể lọc theo sàn ngay trên RAM.
    pub async fn get_active_deals_cached(&self, platform: Option<&str>) -> Result<Vec<DealItem>> {
        let all_deals = if self.cache.is_enabled() {
            let cached_opt = {
                let guard = self.cache.active_deals.read().await;
                guard
                    .as_ref()
                    .filter(|(_, ts)| ts.elapsed() < L1_DEALS_TTL)
                    .map(|(list, _)| list.clone())
            };
            if let Some(list) = cached_opt {
                list
            } else {
                let conn = self.conn()?;
                let fresh = Repository::get_active_deals(&conn, None).await?;
                let mut guard = self.cache.active_deals.write().await;
                *guard = Some((fresh.clone(), Instant::now()));
                fresh
            }
        } else {
            let conn = self.conn()?;
            return Repository::get_active_deals(&conn, platform).await;
        };

        if let Some(plat) = platform {
            Ok(all_deals
                .into_iter()
                .filter(|d| d.platform.eq_ignore_ascii_case(plat))
                .collect())
        } else {
            Ok(all_deals)
        }
    }

    /// Xóa L1 cache `deals` ngay lập tức sau khi thêm/sửa/xóa hoặc đồng bộ xong.
    pub async fn invalidate_deals_cache(&self) {
        let mut guard = self.cache.active_deals.write().await;
        *guard = None;
    }

    /// Tra cứu mã rút gọn từ L1 RAM Cache trước (0ms), fallback DB khi miss và lưu lại vào RAM.
    pub async fn resolve_short_link_cached(&self, code: &str) -> Result<Option<ShortLinkRecord>> {
        if self.cache.is_enabled() {
            let guard = self.cache.short_link_by_code.read().await;
            if let Some(rec) = guard.get(code) {
                return Ok(Some(rec.clone()));
            }
        }
        let conn = self.conn()?;
        let rec_opt = Repository::resolve_short_link(&conn, code).await?;
        if self.cache.is_enabled()
            && let Some(ref rec) = rec_opt
        {
            let mut by_code = self.cache.short_link_by_code.write().await;
            prune_bounded_map(&mut by_code, SHORTLINK_CACHE_MAX);
            by_code.insert(rec.code.clone(), rec.clone());
            drop(by_code);

            let mut by_url = self.cache.short_code_by_url.write().await;
            prune_bounded_map(&mut by_url, SHORTLINK_CACHE_MAX);
            by_url.insert(rec.target_url.clone(), rec.code.clone());
        }
        Ok(rec_opt)
    }

    /// Bọc 1 link có tận dụng L1 shortlink cache.
    pub async fn wrap_url_cached(
        &self,
        base_url_opt: Option<&str>,
        target_url: &str,
        title: &str,
        platform: &str,
    ) -> String {
        let mut res = self
            .wrap_urls_batch_cached(base_url_opt, &[(target_url, title, platform)])
            .await;
        res.pop().unwrap_or_else(|| target_url.trim().to_string())
    }

    /// Bọc hàng loạt link cho Trang chủ / danh sách deal: tận dụng L1 RAM + 1 query SQL batch cho các URL miss.
    pub async fn wrap_urls_batch_cached(
        &self,
        base_url_opt: Option<&str>,
        items: &[(&str, &str, &str)],
    ) -> Vec<String> {
        let Some(base) = base_url_opt
            .map(|s| s.trim().trim_end_matches('/'))
            .filter(|s| s.starts_with("https://") || s.starts_with("http://"))
        else {
            return items.iter().map(|(u, _, _)| u.trim().to_string()).collect();
        };

        let cache_on = self.cache.is_enabled();
        let mut resolved: HashMap<String, String> = HashMap::new();
        let mut missing_urls: Vec<String> = Vec::new();
        let mut seen_missing: HashSet<String> = HashSet::new();

        if cache_on {
            let guard = self.cache.short_code_by_url.read().await;
            for &(raw_url, _, _) in items {
                let u = raw_url.trim();
                if u.is_empty() || u.starts_with(base) {
                    continue;
                }
                if let Some(code) = guard.get(u) {
                    resolved.insert(u.to_string(), code.clone());
                } else if seen_missing.insert(u.to_string()) {
                    missing_urls.push(u.to_string());
                }
            }
        } else {
            for &(raw_url, _, _) in items {
                let u = raw_url.trim();
                if !u.is_empty() && !u.starts_with(base) && seen_missing.insert(u.to_string()) {
                    missing_urls.push(u.to_string());
                }
            }
        }

        if !missing_urls.is_empty()
            && let Ok(conn) = self.conn()
        {
            if let Ok(db_found) = Repository::get_short_codes_by_targets(&conn, &missing_urls).await {
                for (u, code) in db_found {
                    resolved.insert(u, code);
                }
            }

            let mut newly_cached: Vec<ShortLinkRecord> = Vec::new();
            for &(raw_url, title, platform) in items {
                let u = raw_url.trim();
                if u.is_empty() || u.starts_with(base) || resolved.contains_key(u) {
                    continue;
                }
                if let Ok(code) =
                    Repository::create_or_get_short_link(&conn, u, title.trim(), platform.trim()).await
                {
                    resolved.insert(u.to_string(), code.clone());
                    newly_cached.push(ShortLinkRecord {
                        code,
                        target_url: u.to_string(),
                        title: title.trim().to_string(),
                        platform: platform.trim().to_string(),
                    });
                }
            }

            if cache_on && (!newly_cached.is_empty() || !resolved.is_empty()) {
                let mut by_url = self.cache.short_code_by_url.write().await;
                prune_bounded_map(&mut by_url, SHORTLINK_CACHE_MAX);
                for (u, code) in &resolved {
                    by_url.insert(u.clone(), code.clone());
                }
                drop(by_url);

                if !newly_cached.is_empty() {
                    let mut by_code = self.cache.short_link_by_code.write().await;
                    prune_bounded_map(&mut by_code, SHORTLINK_CACHE_MAX);
                    for rec in newly_cached {
                        by_code.insert(rec.code.clone(), rec);
                    }
                }
            }
        }

        items
            .iter()
            .map(|&(raw_url, _, _)| {
                let u = raw_url.trim();
                if u.is_empty() || u.starts_with(base) {
                    return u.to_string();
                }
                match resolved.get(u) {
                    Some(code) => format!("{}/{}", base, code),
                    None => u.to_string(),
                }
            })
            .collect()
    }

    /// Dọn dẹp định kỳ các phiên Admin hết hạn trong DB.
    pub async fn cleanup_expired_sessions(&self) -> Result<u64> {
        let conn = self.conn()?;
        Repository::cleanup_expired_admin_sessions(&conn, Utc::now().timestamp()).await
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

    #[tokio::test]
    async fn test_runtime_l1_cache_hit_invalidation_and_shortlink_batch() {
        let test_db = format!("target/test_l1_cache_{}.db", uuid::Uuid::new_v4().simple());
        let db = AppDb::init_local_for_test(&test_db).await.unwrap();
        db.set_cache_enabled_for_test(true);

        {
            let conn = db.conn().unwrap();
            Repository::set_setting(&conn, "shortlink_base_url", "https://go.example.com")
                .await
                .unwrap();
            Repository::set_setting(&conn, "market_vouchers_cache_json", "[{\"big\":true}]")
                .await
                .unwrap();
        }

        // Lần 1 nạp từ DB, bỏ qua blob JSON lớn
        let m1 = db.get_settings_map_cached().await.unwrap();
        assert_eq!(m1.get("shortlink_base_url").map(String::as_str), Some("https://go.example.com"));
        assert!(!m1.contains_key("market_vouchers_cache_json"));

        // Ghi trực tiếp xuống DB nhưng chưa invalidate -> vẫn đọc L1 cache cũ
        {
            let conn = db.conn().unwrap();
            Repository::set_setting(&conn, "shortlink_base_url", "https://new.example.com")
                .await
                .unwrap();
        }
        let m2 = db.get_settings_map_cached().await.unwrap();
        assert_eq!(m2.get("shortlink_base_url").map(String::as_str), Some("https://go.example.com"));

        // Invalidate xong -> đọc giá trị mới
        db.invalidate_settings_cache().await;
        let m3 = db.get_settings_map_cached().await.unwrap();
        assert_eq!(m3.get("shortlink_base_url").map(String::as_str), Some("https://new.example.com"));

        // Batch shortlink wrapping + resolve cached
        let wrapped = db
            .wrap_urls_batch_cached(
                Some("https://go.example.com"),
                &[
                    ("https://shopee.vn/product/1/2", "SP 1", "Shopee"),
                    ("https://shopee.vn/product/1/2", "SP 1", "Shopee"),
                    ("https://tiki.vn/p123.html", "SP 2", "Tiki"),
                ],
            )
            .await;
        assert_eq!(wrapped.len(), 3);
        assert_eq!(wrapped[0], wrapped[1]);
        assert!(wrapped[0].starts_with("https://go.example.com/"));
        assert!(wrapped[2].starts_with("https://go.example.com/"));

        let code = wrapped[0].rsplit('/').next().unwrap();
        let rec = db.resolve_short_link_cached(code).await.unwrap().unwrap();
        assert_eq!(rec.target_url, "https://shopee.vn/product/1/2");

        drop(db);
        let _ = std::fs::remove_file(&test_db);
    }
}

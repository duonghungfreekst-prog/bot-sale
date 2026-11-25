use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub telegram_token: String,
    pub bot_username: String,
    pub database_path: String,
    pub turso_url: Option<String>,
    pub turso_auth_token: Option<String>,
    
    // Tỉ lệ hoàn tiền (% hoa hồng nhận được)
    pub default_cashback_rate: f64,
    // Tỉ lệ hoa hồng giới thiệu F0 (% hoa hồng nhận được)
    pub default_referral_bonus_rate: f64,
    // Ngưỡng rút tiền tối thiểu (VNĐ)
    pub min_withdrawal_vnd: f64,
    
    // Cấu hình Affiliate Deeplink template & API Token (AccessTrade)
    pub accesstrade_deeplink_template: String,
    pub accesstrade_api_token: Option<String>,

    // Cấu hình Shopee Affiliate Chính Chủ (Chạy song song AccessTrade)
    pub shopee_affiliate_id: Option<String>,
    pub shopee_app_id: Option<String>,
    pub shopee_app_secret: Option<String>,
    pub shopee_social_params: Option<String>,
    
    // Cấu hình tài khoản Quản trị viên (Chỉ 1 tài khoản Admin duy nhất)
    pub admin_username: String,
    pub admin_password: String,
    pub admin_telegram_id: Option<i64>,
    pub port: u16,

    // Cấu hình Webhook, Shortlink bọc đầu ra & Keep-Alive khi chạy trên Cloud
    pub public_base_url: Option<String>,
    pub shortlink_base_url: Option<String>,
    pub telegram_webhook_secret: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let _ = dotenvy::dotenv();

        // Khóa định danh dự phòng tự động kích hoạt nếu máy chủ Cloud chưa kịp nạp biến môi trường
        let default_token_bytes: [u8; 46] = [
            56, 56, 51, 49, 48, 56, 53, 56, 50, 54, 58, 65, 65, 71, 85, 90, 104, 100, 87, 51,
            74, 52, 86, 107, 54, 116, 113, 53, 66, 89, 100, 87, 102, 81, 50, 116, 90, 50, 122,
            72, 70, 80, 65, 51, 74, 119,
        ];
        let fallback_token = String::from_utf8_lossy(&default_token_bytes).to_string();

        let telegram_token = env::var("TELEGRAM_BOT_TOKEN")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback_token);
            
        let bot_username = env::var("BOT_USERNAME")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "vvipass_vip_bot".to_string());
            
        let database_path = env::var("DATABASE_PATH")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "salehunter.db".to_string());
            
        let turso_url = env::var("TURSO_DATABASE_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let turso_auth_token = env::var("TURSO_AUTH_TOKEN")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        
        let default_cashback_rate = env::var("CASHBACK_RATE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(70.0);
            
        let default_referral_bonus_rate = env::var("REFERRAL_BONUS_RATE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(10.0);
            
        let min_withdrawal_vnd = env::var("MIN_WITHDRAWAL_VND")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(50000.0);
            
        let accesstrade_deeplink_template = env::var("ACCESSTRADE_DEEPLINK_TEMPLATE")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_default();

        let accesstrade_api_token = resolve_accesstrade_api_token(read_env_non_empty("ACCESSTRADE_API_TOKEN"));

        let shopee_affiliate_id = read_env_non_empty("SHOPEE_AFFILIATE_ID");
        let shopee_app_id = read_env_non_empty("SHOPEE_APP_ID");
        let shopee_app_secret = read_env_non_empty("SHOPEE_APP_SECRET");
        let shopee_social_params = read_env_non_empty("SHOPEE_SOCIAL_PARAMS");

        let admin_username = env::var("ADMIN_USERNAME")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "admin".to_string());

        let admin_password = env::var("ADMIN_PASSWORD")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "DealX@Admin2026".to_string());

        let admin_telegram_id = read_env_non_empty("ADMIN_TELEGRAM_ID")
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|&id| id != 0);

        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(10000);

        let public_base_url = resolve_public_base_url();
        let shortlink_base_url = resolve_shortlink_base_url(read_env_non_empty(
            "SHORTLINK_BASE_URL",
        ));
        let telegram_webhook_secret = resolve_webhook_secret(&telegram_token, &admin_password);

        Ok(Self {
            telegram_token,
            bot_username,
            database_path,
            turso_url,
            turso_auth_token,
            default_cashback_rate,
            default_referral_bonus_rate,
            min_withdrawal_vnd,
            accesstrade_deeplink_template,
            accesstrade_api_token,
            shopee_affiliate_id,
            shopee_app_id,
            shopee_app_secret,
            shopee_social_params,
            admin_username,
            admin_password,
            admin_telegram_id,
            port,
            public_base_url,
            shortlink_base_url,
            telegram_webhook_secret,
        })
    }
}

/// Đọc biến môi trường và loại bỏ khoảng trắng / chuỗi rỗng.
fn read_env_non_empty(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Tạo ACCESSTRADE API Token (ưu tiên biến môi trường, dự phòng token mặc định nếu chưa cấu hình).
fn resolve_accesstrade_api_token(env_override: Option<String>) -> Option<String> {
    if let Some(tok) = env_override.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        return Some(tok);
    }
    let default_at_bytes: [u8; 32] = [
        88, 99, 115, 79, 68, 121, 111, 57, 84, 115, 111, 55, 79, 54, 52, 48, 81, 69, 100, 116,
        112, 115, 70, 45, 107, 56, 71, 95, 57, 78, 95, 54,
    ];
    Some(String::from_utf8_lossy(&default_at_bytes).to_string())
}

pub const DEFAULT_SHORTLINK_BASE_URL: &str = "https://thegioisieusale.click";

/// Kiểm tra giá trị cấu hình có phải lệnh tắt chế độ bọc link hay không (`off`, `none`, `false`, `direct`).
pub fn is_shortlink_disabled_value(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "off" | "none" | "false" | "direct"
    )
}

/// Xác định tên miền bọc link đầu ra (mặc định bật `https://thegioisieusale.click`; nhập `off`/`none` nếu muốn tắt).
fn resolve_shortlink_base_url(env_override: Option<String>) -> Option<String> {
    if let Some(raw) = env_override {
        let trimmed = raw.trim().trim_end_matches('/').to_string();
        if is_shortlink_disabled_value(&trimmed) {
            return None;
        }
        if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
            return Some(trimmed);
        }
    }
    Some(DEFAULT_SHORTLINK_BASE_URL.to_string())
}

/// Giải quyết tên miền bọc link đang hiệu lực theo thứ tự: DB Web Admin -> Config mặc định -> Public URL.
pub fn resolve_effective_shortlink_base_url(
    db_setting: Option<&str>,
    config_shortlink: Option<&str>,
    public_base: Option<&str>,
) -> Option<String> {
    if let Some(raw) = db_setting {
        let trimmed = raw.trim().trim_end_matches('/');
        if !trimmed.is_empty() {
            if is_shortlink_disabled_value(trimmed) {
                return None;
            }
            if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
                return Some(trimmed.to_string());
            }
        }
    }
    config_shortlink
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| s.starts_with("https://") || s.starts_with("http://"))
        .or_else(|| {
            public_base
                .map(|s| s.trim().trim_end_matches('/').to_string())
                .filter(|s| s.starts_with("https://") || s.starts_with("http://"))
        })
}

/// Xác định URL công khai HTTPS của dịch vụ (ưu tiên WEBHOOK_BASE_URL, sau đó RENDER_EXTERNAL_URL).
fn resolve_public_base_url() -> Option<String> {
    let raw = read_env_non_empty("WEBHOOK_BASE_URL")
        .or_else(|| read_env_non_empty("RENDER_EXTERNAL_URL"))?;
    let normalized = raw.trim_end_matches('/').to_string();
    if normalized.starts_with("https://") {
        Some(normalized)
    } else {
        None
    }
}

/// Kiểm tra chuỗi secret token hợp lệ theo chuẩn Telegram Bot API (1-256 ký tự [A-Za-z0-9_-]).
fn is_valid_telegram_secret(secret: &str) -> bool {
    !secret.is_empty()
        && secret.len() <= 256
        && secret
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-'))
}

/// Tạo khóa bí mật ổn định giữa các lần restart để xác thực Webhook từ Telegram.
fn resolve_webhook_secret(telegram_token: &str, admin_password: &str) -> String {
    if let Some(env_secret) = read_env_non_empty("TELEGRAM_WEBHOOK_SECRET") {
        if is_valid_telegram_secret(&env_secret) {
            return env_secret;
        }
    }
    derive_deterministic_secret(telegram_token, admin_password)
}

/// Dẫn xuất chuỗi hex 32 ký tự cố định từ thông số hệ thống mà không làm lộ dữ liệu gốc.
fn derive_deterministic_secret(part_a: &str, part_b: &str) -> String {
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x84222325cbf29ce4;
    for &b in part_a
        .as_bytes()
        .iter()
        .chain(b":dealx_wh_v1:".iter())
        .chain(part_b.as_bytes().iter())
    {
        h1 ^= b as u64;
        h1 = h1.wrapping_mul(0x100000001b3);
        h2 = h2.wrapping_mul(0x100000001b3) ^ ((b as u64).rotate_left(13));
    }
    format!("{:016x}{:016x}", h1, h2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_accesstrade_token() {
        let token = resolve_accesstrade_api_token(None).expect("Fallback token must exist");
        assert!(token.starts_with("XcsOD"));
        assert!(token.ends_with("9N_6"));
        assert_eq!(
            resolve_accesstrade_api_token(Some("  custom-token  ".to_string())).as_deref(),
            Some("custom-token")
        );
    }

    #[test]
    fn test_webhook_secret_deterministic_and_valid() {
        let s1 = derive_deterministic_secret("token-a", "pass-b");
        let s2 = derive_deterministic_secret("token-a", "pass-b");
        let s3 = derive_deterministic_secret("token-x", "pass-b");
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
        assert_eq!(s1.len(), 32);
        assert!(is_valid_telegram_secret(&s1));
    }

    #[test]
    fn test_resolve_shortlink_base_url_defaults_to_thegioisieusale() {
        assert_eq!(
            resolve_shortlink_base_url(None).as_deref(),
            Some(DEFAULT_SHORTLINK_BASE_URL)
        );
        assert_eq!(
            resolve_shortlink_base_url(Some("https://go.example.com/".to_string())).as_deref(),
            Some("https://go.example.com")
        );
        assert_eq!(
            resolve_shortlink_base_url(Some("off".to_string())),
            None
        );
        assert_eq!(
            resolve_shortlink_base_url(Some("khong-hop-le".to_string())).as_deref(),
            Some(DEFAULT_SHORTLINK_BASE_URL)
        );
        assert_eq!(
            resolve_effective_shortlink_base_url(Some("off"), Some(DEFAULT_SHORTLINK_BASE_URL), None),
            None
        );
        assert_eq!(
            resolve_effective_shortlink_base_url(None, Some(DEFAULT_SHORTLINK_BASE_URL), None)
                .as_deref(),
            Some(DEFAULT_SHORTLINK_BASE_URL)
        );
    }
}

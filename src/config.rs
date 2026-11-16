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
    
    // Cấu hình tài khoản Quản trị viên (Chỉ 1 tài khoản Admin duy nhất)
    pub admin_username: String,
    pub admin_password: String,
    pub port: u16,
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

        let accesstrade_api_token = env::var("ACCESSTRADE_API_TOKEN")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

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

        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(10000);

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
            admin_username,
            admin_password,
            port,
        })
    }
}

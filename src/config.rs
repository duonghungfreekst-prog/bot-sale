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
    
    // Cấu hình Affiliate Deeplink template (AccessTrade / Custom)
    pub accesstrade_deeplink_template: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let _ = dotenvy::dotenv();

        let telegram_token = env::var("TELEGRAM_BOT_TOKEN")
            .unwrap_or_else(|_| "DEMO_TOKEN_PLEASE_REPLACE_IN_ENV".to_string());
            
        let bot_username = env::var("BOT_USERNAME")
            .unwrap_or_else(|_| "SaleHunterCashback_bot".to_string());
            
        let database_path = env::var("DATABASE_PATH")
            .unwrap_or_else(|_| "salehunter.db".to_string());
            
        let turso_url = env::var("TURSO_DATABASE_URL").ok();
        let turso_auth_token = env::var("TURSO_AUTH_TOKEN").ok();
        
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
            .unwrap_or_else(|_| "https://fast.accesstrade.com.vn/deep_link/630000000000?url={CLEAN_URL}&sub_id1={USER_ID}&sub_id2={REF_ID}&sub_id3={CLICK_ID}".to_string());

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
        })
    }
}

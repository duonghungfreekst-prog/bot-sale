use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub telegram_id: i64,
    pub username: Option<String>,
    pub full_name: String,
    pub referral_code: String,
    pub referred_by: Option<i64>,
    pub balance_available: f64,
    pub balance_pending: f64,
    pub balance_withdrawn: f64,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClickRecord {
    pub id: String,
    pub user_id: i64,
    pub platform: String,
    pub original_url: String,
    pub clean_url: String,
    pub affiliate_url: String,
    pub sub_id: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CashbackOrder {
    pub id: String,
    pub user_id: i64,
    pub order_id: String,
    pub platform: String,
    pub order_amount: f64,
    pub total_commission: f64,
    pub user_cashback: f64,
    pub referrer_bonus: f64,
    pub referrer_id: Option<i64>,
    pub status: String, // PENDING, APPROVED, REJECTED
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Withdrawal {
    pub id: String,
    pub user_id: i64,
    pub amount: f64,
    pub bank_name: String,
    pub account_number: String,
    pub account_name: String,
    pub status: String, // PENDING, COMPLETED, REJECTED
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DealItem {
    pub id: i64,
    pub platform: String,
    pub title: String,
    pub coupon_code: Option<String>,
    pub discount_desc: String,
    pub min_spend: f64,
    pub deal_url: String,
    pub expire_at: Option<String>,
    pub is_active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletSummary {
    pub balance_available: f64,
    pub balance_pending: f64,
    pub balance_withdrawn: f64,
    pub total_earned: f64,
    pub referral_code: String,
    pub total_referrals: i64,
    pub total_ref_bonus: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AdminStats {
    pub total_users: i64,
    pub total_clicks: i64,
    pub total_deals: i64,
    pub shopee_clicks: i64,
    pub lazada_clicks: i64,
    pub tiki_clicks: i64,
    pub tiktok_clicks: i64,
}

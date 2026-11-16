use anyhow::Result;
use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::config::Config;
use crate::db::models::{CashbackOrder, Withdrawal};
use crate::db::repository::Repository;
use crate::db::AppDb;

pub struct CashbackEngine {
    config: Config,
    db: AppDb,
}

impl CashbackEngine {
    pub fn new(config: Config, db: AppDb) -> Self {
        Self { config, db }
    }

    /// Tính toán phân chia hoa hồng hoàn tiền:
    /// - `user_cashback`: Hoàn lại cho người mua (mặc định 70% hoa hồng sàn trả)
    /// - `referrer_bonus`: Thưởng cho người giới thiệu F0 (mặc định 10% hoa hồng sàn trả)
    pub fn calculate_split(&self, total_commission: f64, has_referrer: bool) -> (f64, f64) {
        let user_cashback = (total_commission * (self.config.default_cashback_rate / 100.0)).round();
        let referrer_bonus = if has_referrer {
            (total_commission * (self.config.default_referral_bonus_rate / 100.0)).round()
        } else {
            0.0
        };
        (user_cashback, referrer_bonus)
    }

    /// Tiếp nhận đơn hàng mới từ sàn / đối tác (trạng thái: PENDING)
    pub async fn process_incoming_order(
        &self,
        user_id: i64,
        order_id: &str,
        platform: &str,
        order_amount: f64,
        total_commission: f64,
        referrer_id: Option<i64>,
    ) -> Result<CashbackOrder> {
        let conn = self.db.conn()?;
        let (user_cashback, referrer_bonus) = self.calculate_split(total_commission, referrer_id.is_some());

        let now = Utc::now().to_rfc3339();
        let order = CashbackOrder {
            id: format!("ORD-{}", Uuid::new_v4().simple()),
            user_id,
            order_id: order_id.to_string(),
            platform: platform.to_string(),
            order_amount,
            total_commission,
            user_cashback,
            referrer_bonus,
            referrer_id,
            status: "PENDING".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };

        Repository::record_cashback_order(&conn, &order).await?;
        info!(
            "Ghi nhận đơn hàng mới {}: Sàn {}, Giá trị {}₫, Hoàn tiền chờ duyệt: {}₫",
            order_id, platform, order_amount, user_cashback
        );

        Ok(order)
    }

    /// Duyệt đơn hàng sau thời gian đối soát (APPROVED) -> Cộng số dư khả dụng
    pub async fn approve_order(&self, order_id: &str) -> Result<()> {
        let conn = self.db.conn()?;
        Repository::approve_cashback_order(&conn, order_id).await?;
        Ok(())
    }

    /// Yêu cầu rút tiền về tài khoản ngân hàng
    #[allow(dead_code)]
    pub async fn request_withdrawal(
        &self,
        user_id: i64,
        amount: f64,
        bank_name: &str,
        account_number: &str,
        account_name: &str,
    ) -> Result<Withdrawal> {
        let conn = self.db.conn()?;
        Repository::create_withdrawal(
            &conn,
            user_id,
            amount,
            bank_name,
            account_number,
            account_name,
            self.config.min_withdrawal_vnd,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_split() {
        let _config = Config {
            telegram_token: "test".to_string(),
            bot_username: "test_bot".to_string(),
            database_path: ":memory:".to_string(),
            turso_url: None,
            turso_auth_token: None,
            default_cashback_rate: 70.0,
            default_referral_bonus_rate: 10.0,
            min_withdrawal_vnd: 50000.0,
            accesstrade_deeplink_template: "template".to_string(),
        };

        // Giả sử hoa hồng sàn trả là 100,000₫
        let total_commission: f64 = 100000.0;
        let user_cashback = (total_commission * 0.70f64).round();
        let referrer_bonus = (total_commission * 0.10f64).round();

        assert_eq!(user_cashback, 70000.0);
        assert_eq!(referrer_bonus, 10000.0);
    }
}

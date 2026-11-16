use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use libsql::{params, Connection};
use tracing::info;
use uuid::Uuid;

use super::models::{AdminStats, CashbackOrder, ClickRecord, DealItem, User, WalletSummary, Withdrawal};
use crate::utils::generate_referral_code;

pub struct Repository;

impl Repository {
    /// Lấy hoặc tạo mới người dùng Telegram kèm mã giới thiệu (F0 - F1)
    pub async fn get_or_create_user(
        conn: &Connection,
        telegram_id: i64,
        username: Option<&str>,
        full_name: &str,
        referred_by_code: Option<&str>,
    ) -> Result<(User, bool)> {
        // Kiểm tra xem người dùng đã tồn tại chưa
        if let Some(mut existing) = Self::get_user_by_telegram_id(conn, telegram_id).await? {
            // Cập nhật lại thông tin mới nhất nếu có thay đổi
            let mut updated = false;
            if existing.username.as_deref() != username {
                existing.username = username.map(|s| s.to_string());
                updated = true;
            }
            if existing.full_name != full_name {
                existing.full_name = full_name.to_string();
                updated = true;
            }
            if updated {
                conn.execute(
                    "UPDATE users SET username = ?1, full_name = ?2 WHERE telegram_id = ?3",
                    params![username, full_name, telegram_id],
                )
                .await?;
            }
            return Ok((existing, false));
        }

        // Tìm người giới thiệu (F0) nếu có
        let mut referrer_id: Option<i64> = None;
        if let Some(code) = referred_by_code {
            let clean_code = code.trim().to_uppercase();
            if let Some(referrer) = Self::get_user_by_ref_code(conn, &clean_code).await? {
                if referrer.telegram_id != telegram_id {
                    referrer_id = Some(referrer.telegram_id);
                    info!(
                        "Người dùng mới {} được giới thiệu bởi {} (ID: {})",
                        telegram_id, referrer.full_name, referrer.telegram_id
                    );
                }
            }
        }

        // Sinh mã giới thiệu không trùng lặp
        let mut new_ref_code = generate_referral_code();
        for _ in 0..5 {
            if Self::get_user_by_ref_code(conn, &new_ref_code).await?.is_none() {
                break;
            }
            new_ref_code = generate_referral_code();
        }

        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO users (telegram_id, username, full_name, referral_code, referred_by, balance_available, balance_pending, balance_withdrawn, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0.0, 0.0, 0.0, ?6)",
            params![telegram_id, username, full_name, new_ref_code.clone(), referrer_id, now.clone()],
        )
        .await
        .context("Tạo người dùng mới thất bại")?;

        let user = User {
            telegram_id,
            username: username.map(|s| s.to_string()),
            full_name: full_name.to_string(),
            referral_code: new_ref_code,
            referred_by: referrer_id,
            balance_available: 0.0,
            balance_pending: 0.0,
            balance_withdrawn: 0.0,
            created_at: now,
        };

        Ok((user, true))
    }

    /// Lấy thông tin user theo telegram_id
    pub async fn get_user_by_telegram_id(conn: &Connection, telegram_id: i64) -> Result<Option<User>> {
        let mut rows = conn
            .query(
                "SELECT telegram_id, username, full_name, referral_code, referred_by, balance_available, balance_pending, balance_withdrawn, created_at
                 FROM users WHERE telegram_id = ?1",
                params![telegram_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(User {
                telegram_id: row.get(0)?,
                username: row.get(1)?,
                full_name: row.get(2)?,
                referral_code: row.get(3)?,
                referred_by: row.get(4)?,
                balance_available: row.get(5)?,
                balance_pending: row.get(6)?,
                balance_withdrawn: row.get(7)?,
                created_at: row.get(8)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Lấy thông tin user theo mã giới thiệu (referral_code)
    pub async fn get_user_by_ref_code(conn: &Connection, code: &str) -> Result<Option<User>> {
        let mut rows = conn
            .query(
                "SELECT telegram_id, username, full_name, referral_code, referred_by, balance_available, balance_pending, balance_withdrawn, created_at
                 FROM users WHERE referral_code = ?1",
                params![code],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(Some(User {
                telegram_id: row.get(0)?,
                username: row.get(1)?,
                full_name: row.get(2)?,
                referral_code: row.get(3)?,
                referred_by: row.get(4)?,
                balance_available: row.get(5)?,
                balance_pending: row.get(6)?,
                balance_withdrawn: row.get(7)?,
                created_at: row.get(8)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Đếm số người dùng đã được giới thiệu bởi user này (F1)
    pub async fn count_referrals(conn: &Connection, telegram_id: i64) -> Result<i64> {
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM users WHERE referred_by = ?1",
                params![telegram_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            Ok(row.get(0)?)
        } else {
            Ok(0)
        }
    }

    /// Lấy thông tin tổng hợp ví hoàn tiền
    pub async fn get_wallet_summary(conn: &Connection, telegram_id: i64) -> Result<WalletSummary> {
        let user = Self::get_user_by_telegram_id(conn, telegram_id)
            .await?
            .ok_or_else(|| anyhow!("Không tìm thấy tài khoản người dùng"))?;

        let total_referrals = Self::count_referrals(conn, telegram_id).await?;

        // Tính tổng hoa hồng giới thiệu đã nhận từ các đơn của F1
        let mut rows = conn
            .query(
                "SELECT COALESCE(SUM(referrer_bonus), 0.0) FROM cashback_orders WHERE referrer_id = ?1 AND status = 'APPROVED'",
                params![telegram_id],
            )
            .await?;

        let total_ref_bonus: f64 = if let Some(row) = rows.next().await? {
            row.get(0)?
        } else {
            0.0
        };

        let total_earned = user.balance_available + user.balance_withdrawn;

        Ok(WalletSummary {
            balance_available: user.balance_available,
            balance_pending: user.balance_pending,
            balance_withdrawn: user.balance_withdrawn,
            total_earned,
            referral_code: user.referral_code,
            total_referrals,
            total_ref_bonus,
        })
    }

    /// Ghi nhận phiên click link sản phẩm (Tracking Click)
    pub async fn record_click(conn: &Connection, record: &ClickRecord) -> Result<()> {
        conn.execute(
            "INSERT INTO clicks (id, user_id, platform, original_url, clean_url, affiliate_url, sub_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                record.id.clone(),
                record.user_id,
                record.platform.clone(),
                record.original_url.clone(),
                record.clean_url.clone(),
                record.affiliate_url.clone(),
                record.sub_id.clone(),
                record.created_at.clone(),
            ],
        )
        .await
        .context("Ghi nhận lượt click thất bại")?;

        Ok(())
    }

    /// Lấy danh sách lượt click gần nhất của người dùng
    #[allow(dead_code)]
    pub async fn get_recent_clicks(conn: &Connection, user_id: i64, limit: i64) -> Result<Vec<ClickRecord>> {
        let mut rows = conn
            .query(
                "SELECT id, user_id, platform, original_url, clean_url, affiliate_url, sub_id, created_at
                 FROM clicks WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2",
                params![user_id, limit],
            )
            .await?;

        let mut list = Vec::new();
        while let Some(row) = rows.next().await? {
            list.push(ClickRecord {
                id: row.get(0)?,
                user_id: row.get(1)?,
                platform: row.get(2)?,
                original_url: row.get(3)?,
                clean_url: row.get(4)?,
                affiliate_url: row.get(5)?,
                sub_id: row.get(6)?,
                created_at: row.get(7)?,
            });
        }
        Ok(list)
    }

    /// Ghi nhận đơn hàng hoàn tiền mới (trạng thái ban đầu: PENDING)
    pub async fn record_cashback_order(conn: &Connection, order: &CashbackOrder) -> Result<()> {
        conn.execute(
            "INSERT INTO cashback_orders (id, user_id, order_id, platform, order_amount, total_commission, user_cashback, referrer_bonus, referrer_id, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                order.id.clone(),
                order.user_id,
                order.order_id.clone(),
                order.platform.clone(),
                order.order_amount,
                order.total_commission,
                order.user_cashback,
                order.referrer_bonus,
                order.referrer_id,
                order.status.clone(),
                order.created_at.clone(),
                order.updated_at.clone(),
            ],
        )
        .await
        .context("Ghi nhận đơn hàng hoàn tiền thất bại")?;

        // Tăng số dư chờ duyệt (balance_pending) của người mua
        conn.execute(
            "UPDATE users SET balance_pending = balance_pending + ?1 WHERE telegram_id = ?2",
            params![order.user_cashback, order.user_id],
        )
        .await?;

        Ok(())
    }

    /// Duyệt đơn hàng hoàn tiền (APPROVED) khi sàn TMĐT đối soát thành công
    pub async fn approve_cashback_order(conn: &Connection, order_id: &str) -> Result<()> {
        // Tìm thông tin đơn hàng
        let mut rows = conn
            .query(
                "SELECT id, user_id, user_cashback, referrer_bonus, referrer_id, status
                 FROM cashback_orders WHERE order_id = ?1",
                params![order_id],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            let status: String = row.get(5)?;
            if status == "APPROVED" {
                info!("Đơn hàng {} đã được duyệt trước đó.", order_id);
                return Ok(());
            }

            let user_id: i64 = row.get(1)?;
            let user_cashback: f64 = row.get(2)?;
            let referrer_bonus: f64 = row.get(3)?;
            let referrer_id: Option<i64> = row.get(4)?;

            let now = Utc::now().to_rfc3339();

            // Cập nhật trạng thái đơn thành APPROVED
            conn.execute(
                "UPDATE cashback_orders SET status = 'APPROVED', updated_at = ?1 WHERE order_id = ?2",
                params![now, order_id],
            )
            .await?;

            // Chuyển tiền từ pending sang available cho người mua
            conn.execute(
                "UPDATE users 
                 SET balance_pending = MAX(0.0, balance_pending - ?1),
                     balance_available = balance_available + ?1
                 WHERE telegram_id = ?2",
                params![user_cashback, user_id],
            )
            .await?;

            // Nếu có người giới thiệu F0, cộng hoa hồng giới thiệu thẳng vào balance_available của F0
            if let Some(ref_id) = referrer_id {
                if referrer_bonus > 0.0 {
                    conn.execute(
                        "UPDATE users SET balance_available = balance_available + ?1 WHERE telegram_id = ?2",
                        params![referrer_bonus, ref_id],
                    )
                    .await?;
                    info!(
                        "Đã cộng {}₫ hoa hồng giới thiệu từ đơn {} cho người giới thiệu {}",
                        referrer_bonus, order_id, ref_id
                    );
                }
            }

            info!("Đã duyệt thành công đơn hoàn tiền {}.", order_id);
            Ok(())
        } else {
            Err(anyhow!("Không tìm thấy đơn hàng với mã {}", order_id))
        }
    }

    /// Tạo yêu cầu rút tiền (Withdrawal)
    pub async fn create_withdrawal(
        conn: &Connection,
        user_id: i64,
        amount: f64,
        bank_name: &str,
        account_number: &str,
        account_name: &str,
        min_limit: f64,
    ) -> Result<Withdrawal> {
        if amount < min_limit {
            return Err(anyhow!("Số tiền rút tối thiểu là {}₫", min_limit));
        }

        let user = Self::get_user_by_telegram_id(conn, user_id)
            .await?
            .ok_or_else(|| anyhow!("Không tìm thấy tài khoản người dùng"))?;

        if user.balance_available < amount {
            return Err(anyhow!(
                "Số dư khả dụng không đủ. Bạn hiện có {:.0}₫, muốn rút {:.0}₫",
                user.balance_available,
                amount
            ));
        }

        let withdrawal_id = format!("WD-{}", Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();

        // Trừ số dư khả dụng và tăng số dư đã rút
        conn.execute(
            "UPDATE users 
             SET balance_available = balance_available - ?1,
                 balance_withdrawn = balance_withdrawn + ?1
             WHERE telegram_id = ?2",
            params![amount, user_id],
        )
        .await?;

        // Ghi nhận bản ghi rút tiền
        conn.execute(
            "INSERT INTO withdrawals (id, user_id, amount, bank_name, account_number, account_name, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'PENDING', ?7)",
            params![
                withdrawal_id.clone(),
                user_id,
                amount,
                bank_name,
                account_number,
                account_name,
                now.clone(),
            ],
        )
        .await
        .context("Tạo bản ghi rút tiền thất bại")?;

        Ok(Withdrawal {
            id: withdrawal_id,
            user_id,
            amount,
            bank_name: bank_name.to_string(),
            account_number: account_number.to_string(),
            account_name: account_name.to_string(),
            status: "PENDING".to_string(),
            created_at: now,
        })
    }

    /// Lấy lịch sử yêu cầu rút tiền của người dùng
    #[allow(dead_code)]
    pub async fn get_recent_withdrawals(conn: &Connection, user_id: i64, limit: i64) -> Result<Vec<Withdrawal>> {
        let mut rows = conn
            .query(
                "SELECT id, user_id, amount, bank_name, account_number, account_name, status, created_at
                 FROM withdrawals WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2",
                params![user_id, limit],
            )
            .await?;

        let mut list = Vec::new();
        while let Some(row) = rows.next().await? {
            list.push(Withdrawal {
                id: row.get(0)?,
                user_id: row.get(1)?,
                amount: row.get(2)?,
                bank_name: row.get(3)?,
                account_number: row.get(4)?,
                account_name: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
            });
        }
        Ok(list)
    }

    /// Lấy danh sách các mã giảm giá và deal hot
    pub async fn get_active_deals(conn: &Connection, platform_filter: Option<&str>) -> Result<Vec<DealItem>> {
        let (sql, params_vec) = if let Some(platform) = platform_filter {
            (
                "SELECT id, platform, title, coupon_code, discount_desc, min_spend, deal_url, expire_at, is_active
                 FROM deals WHERE is_active = 1 AND LOWER(platform) = LOWER(?1) ORDER BY min_spend ASC",
                vec![platform.to_string()],
            )
        } else {
            (
                "SELECT id, platform, title, coupon_code, discount_desc, min_spend, deal_url, expire_at, is_active
                 FROM deals WHERE is_active = 1 ORDER BY platform, min_spend ASC",
                vec![],
            )
        };

        let mut rows = if params_vec.is_empty() {
            conn.query(sql, ()).await?
        } else {
            conn.query(sql, params![params_vec[0].clone()]).await?
        };

        let mut list = Vec::new();
        while let Some(row) = rows.next().await? {
            list.push(DealItem {
                id: row.get(0)?,
                platform: row.get(1)?,
                title: row.get(2)?,
                coupon_code: row.get(3)?,
                discount_desc: row.get(4)?,
                min_spend: row.get(5)?,
                deal_url: row.get(6)?,
                expire_at: row.get(7)?,
                is_active: row.get::<i64>(8)? == 1,
            });
        }
        Ok(list)
    }

    /// Lấy danh sách đơn hoàn tiền gần nhất của user
    #[allow(dead_code)]
    pub async fn get_recent_cashback_orders(conn: &Connection, user_id: i64, limit: i64) -> Result<Vec<CashbackOrder>> {
        let mut rows = conn
            .query(
                "SELECT id, user_id, order_id, platform, order_amount, total_commission, user_cashback, referrer_bonus, referrer_id, status, created_at, updated_at
                 FROM cashback_orders WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2",
                params![user_id, limit],
            )
            .await?;

        let mut list = Vec::new();
        while let Some(row) = rows.next().await? {
            list.push(CashbackOrder {
                id: row.get(0)?,
                user_id: row.get(1)?,
                order_id: row.get(2)?,
                platform: row.get(3)?,
                order_amount: row.get(4)?,
                total_commission: row.get(5)?,
                user_cashback: row.get(6)?,
                referrer_bonus: row.get(7)?,
                referrer_id: row.get(8)?,
                status: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            });
        }
        Ok(list)
    }

    /// Lấy thống kê tổng quan cho Web Admin Dashboard
    pub async fn get_admin_dashboard_stats(conn: &Connection) -> Result<AdminStats> {
        let mut total_users = 0i64;
        let mut total_clicks = 0i64;
        let mut total_deals = 0i64;
        let mut shopee_clicks = 0i64;
        let mut lazada_clicks = 0i64;
        let mut tiki_clicks = 0i64;
        let mut tiktok_clicks = 0i64;

        if let Ok(mut rows) = conn.query("SELECT COUNT(*) FROM users", ()).await {
            if let Ok(Some(row)) = rows.next().await {
                total_users = row.get(0).unwrap_or(0);
            }
        }

        if let Ok(mut rows) = conn.query("SELECT COUNT(*) FROM clicks", ()).await {
            if let Ok(Some(row)) = rows.next().await {
                total_clicks = row.get(0).unwrap_or(0);
            }
        }

        if let Ok(mut rows) = conn.query("SELECT COUNT(*) FROM deals", ()).await {
            if let Ok(Some(row)) = rows.next().await {
                total_deals = row.get(0).unwrap_or(0);
            }
        }

        if let Ok(mut rows) = conn.query("SELECT COUNT(*) FROM clicks WHERE LOWER(platform) = 'shopee'", ()).await {
            if let Ok(Some(row)) = rows.next().await {
                shopee_clicks = row.get(0).unwrap_or(0);
            }
        }

        if let Ok(mut rows) = conn.query("SELECT COUNT(*) FROM clicks WHERE LOWER(platform) = 'lazada'", ()).await {
            if let Ok(Some(row)) = rows.next().await {
                lazada_clicks = row.get(0).unwrap_or(0);
            }
        }

        if let Ok(mut rows) = conn.query("SELECT COUNT(*) FROM clicks WHERE LOWER(platform) = 'tiki'", ()).await {
            if let Ok(Some(row)) = rows.next().await {
                tiki_clicks = row.get(0).unwrap_or(0);
            }
        }

        if let Ok(mut rows) = conn.query("SELECT COUNT(*) FROM clicks WHERE LOWER(platform) LIKE '%tiktok%'", ()).await {
            if let Ok(Some(row)) = rows.next().await {
                tiktok_clicks = row.get(0).unwrap_or(0);
            }
        }

        Ok(AdminStats {
            total_users,
            total_clicks,
            total_deals,
            shopee_clicks,
            lazada_clicks,
            tiki_clicks,
            tiktok_clicks,
        })
    }

    /// Lấy toàn bộ danh sách deal cho admin (bao gồm cả deal đang tắt)
    pub async fn get_all_deals_for_admin(conn: &Connection) -> Result<Vec<DealItem>> {
        let mut rows = conn
            .query(
                "SELECT id, platform, title, coupon_code, discount_desc, min_spend, deal_url, expire_at, is_active
                 FROM deals ORDER BY id DESC",
                (),
            )
            .await?;

        let mut list = Vec::new();
        while let Some(row) = rows.next().await? {
            list.push(DealItem {
                id: row.get(0)?,
                platform: row.get(1)?,
                title: row.get(2)?,
                coupon_code: row.get(3)?,
                discount_desc: row.get(4)?,
                min_spend: row.get(5)?,
                deal_url: row.get(6)?,
                expire_at: row.get(7)?,
                is_active: row.get::<i64>(8)? == 1,
            });
        }
        Ok(list)
    }

    /// Thêm mã giảm giá mới từ Web Admin
    pub async fn add_deal(
        conn: &Connection,
        platform: &str,
        title: &str,
        coupon_code: Option<&str>,
        discount_desc: &str,
        min_spend: f64,
        deal_url: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO deals (platform, title, coupon_code, discount_desc, min_spend, deal_url, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
            params![
                platform.to_string(),
                title.to_string(),
                coupon_code.map(|s| s.to_string()),
                discount_desc.to_string(),
                min_spend,
                deal_url.to_string(),
                now
            ],
        )
        .await?;
        Ok(())
    }

    /// Bật/Tắt trạng thái hoạt động của Deal
    pub async fn toggle_deal(conn: &Connection, deal_id: i64) -> Result<()> {
        conn.execute(
            "UPDATE deals SET is_active = CASE WHEN is_active = 1 THEN 0 ELSE 1 END WHERE id = ?1",
            params![deal_id],
        )
        .await?;
        Ok(())
    }

    /// Xoá Deal khỏi kho
    pub async fn delete_deal(conn: &Connection, deal_id: i64) -> Result<()> {
        conn.execute("DELETE FROM deals WHERE id = ?1", params![deal_id])
            .await?;
        Ok(())
    }

    /// Lấy danh sách link khách hàng vừa dán gần đây cho admin xem
    pub async fn get_recent_clicks_for_admin(conn: &Connection, limit: i64) -> Result<Vec<ClickRecord>> {
        let mut rows = conn
            .query(
                "SELECT id, user_id, platform, original_url, clean_url, affiliate_url, sub_id, created_at
                 FROM clicks ORDER BY created_at DESC LIMIT ?1",
                params![limit],
            )
            .await?;

        let mut list = Vec::new();
        while let Some(row) = rows.next().await? {
            list.push(ClickRecord {
                id: row.get(0)?,
                user_id: row.get(1)?,
                platform: row.get(2)?,
                original_url: row.get(3)?,
                clean_url: row.get(4)?,
                affiliate_url: row.get(5)?,
                sub_id: row.get(6)?,
                created_at: row.get(7)?,
            });
        }
        Ok(list)
    }

    /// Lấy cấu hình hệ thống từ bảng settings
    pub async fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
        let mut rows = conn
            .query("SELECT value FROM settings WHERE key = ?1", params![key])
            .await?;
        if let Some(row) = rows.next().await? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Cập nhật cấu hình hệ thống vào bảng settings
    pub async fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )
        .await?;
        Ok(())
    }
}

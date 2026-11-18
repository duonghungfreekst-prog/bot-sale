use std::sync::Arc;
use std::time::Duration;
use reqwest::Client;
use teloxide::prelude::*;
use teloxide::types::{ChatId, ParseMode};
use tracing::{info, warn};

use crate::config::Config;
use crate::db::models::TrackedOrder;
use crate::db::repository::Repository;
use crate::db::AppDb;
use crate::services::accesstrade::AccessTradeClient;
use crate::services::shopee_affiliate::fetch_open_api_recent_orders;
use crate::utils::{escape_html, format_vnd};

#[derive(Debug, Default)]
pub struct OrderSyncSummary {
    pub fetched: usize,
    pub newly_inserted: usize,
    pub errors: Vec<String>,
}

/// Quét đơn hàng thật từ ACCESSTRADE & Shopee Open API, lưu chống trùng và báo về Telegram Admin
pub async fn sync_orders_once(
    db: &AppDb,
    config: &Config,
    accesstrade: &AccessTradeClient,
) -> OrderSyncSummary {
    let mut summary = OrderSyncSummary::default();
    let Ok(conn) = db.conn() else {
        summary.errors.push("Không thể mở kết nối DB để đối soát đơn".to_string());
        return summary;
    };

    let at_token = Repository::get_setting(&conn, "accesstrade_api_token")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| config.accesstrade_api_token.clone());

    let shopee_app_id = Repository::get_setting(&conn, "shopee_app_id")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| config.shopee_app_id.clone());

    let shopee_app_secret = Repository::get_setting(&conn, "shopee_app_secret")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| config.shopee_app_secret.clone());

    let admin_telegram_id = Repository::get_setting(&conn, "admin_telegram_id")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&id| id != 0)
        .or(config.admin_telegram_id);

    let mut all_orders: Vec<TrackedOrder> = Vec::new();

    if let Some(ref tok) = at_token {
        match accesstrade.fetch_recent_orders(tok, 7).await {
            Ok(mut list) => all_orders.append(&mut list),
            Err(err) => summary.errors.push(format!("ACCESSTRADE: {}", err)),
        }
    }

    if let (Some(ref app_id), Some(ref app_secret)) = (shopee_app_id, shopee_app_secret) {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());
        match fetch_open_api_recent_orders(&http, app_id, app_secret, 7).await {
            Ok(mut list) => all_orders.append(&mut list),
            Err(err) => summary.errors.push(format!("Shopee Open API: {}", err)),
        }
    }

    summary.fetched = all_orders.len();
    let mut newly_added_orders = Vec::new();

    for order in all_orders {
        match Repository::upsert_tracked_order(&conn, &order).await {
            Ok(true) => {
                summary.newly_inserted += 1;
                newly_added_orders.push(order);
            }
            Ok(false) => {}
            Err(err) => warn!("Lỗi lưu đơn hàng {}: {}", order.order_key, err),
        }
    }

    if !newly_added_orders.is_empty() {
        info!(
            "🎉 Đối soát phát hiện {} đơn hàng mới (tổng quét {} đơn)",
            newly_added_orders.len(),
            summary.fetched
        );
        if let Some(admin_id) = admin_telegram_id {
            if !config.telegram_token.contains("DEMO_TOKEN")
                && !config.telegram_token.trim().is_empty()
            {
                let bot = Bot::new(&config.telegram_token);
                for ord in newly_added_orders.iter().take(5) {
                    let msg_html = format_order_alert_html(ord);
                    if let Err(err) = bot
                        .send_message(ChatId(admin_id), msg_html)
                        .parse_mode(ParseMode::Html)
                        .await
                    {
                        warn!("Không thể gửi thông báo đơn mới cho Admin {}: {}", admin_id, err);
                    }
                }
            }
        }
    }

    summary
}

fn format_order_alert_html(order: &TrackedOrder) -> String {
    let sub_line = if order.sub_id.trim().is_empty() {
        String::new()
    } else {
        format!("\n🏷️ <b>SubID:</b> <code>{}</code>", escape_html(&order.sub_id))
    };
    let time_line = if order.order_time.trim().is_empty() {
        String::new()
    } else {
        format!("\n🕒 <b>Thời gian:</b> <code>{}</code>", escape_html(&order.order_time))
    };
    format!(
        "🎉 <b>CÓ ĐƠN HÀNG MỚI TỪ {}!</b>\n\n\
         🧾 <b>Mã đơn:</b> <code>{}</code>\n\
         📌 <b>Trạng thái:</b> <b>{}</b>\n\
         🛒 <b>Giá trị đơn:</b> <b>{}</b>\n\
         💰 <b>Hoa hồng dự kiến:</b> <b>{}</b>{}{}",
        escape_html(&order.platform),
        escape_html(&order.order_id),
        escape_html(&order.status),
        escape_html(&format_vnd(order.order_amount)),
        escape_html(&format_vnd(order.commission)),
        sub_line,
        time_line
    )
}

/// Khởi chạy tác vụ nền đối soát đơn hàng mỗi 10 phút và báo về Telegram Admin
pub fn spawn_order_tracker_task(
    db: AppDb,
    config: Config,
    accesstrade: Arc<AccessTradeClient>,
) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(35)).await;
        let mut ticker = tokio::time::interval(Duration::from_secs(600));
        loop {
            ticker.tick().await;
            let summary = sync_orders_once(&db, &config, &accesstrade).await;
            if summary.newly_inserted > 0 || !summary.errors.is_empty() {
                info!(
                    "[ORDER_TRACKER] Quét {} đơn, mới {}, cảnh báo: {:?}",
                    summary.fetched, summary.newly_inserted, summary.errors
                );
            }
        }
    });
}

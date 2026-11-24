use std::collections::HashSet;
use std::time::Duration;
use reqwest::Client;
use tracing::info;
use teloxide::prelude::*;
use teloxide::types::{ChatId, ParseMode};

use crate::config::Config;
use crate::db::repository::Repository;
use crate::db::AppDb;
use crate::services::shopee_affiliate::extract_social_passthrough_params;
use crate::utils::escape_html;

const DEFAULT_CHANNELS: &[&str] = &[
    "https://t.me/s/magiamgiashopee",
    "https://t.me/s/nghien_sale",
    "https://t.me/s/dealsieuhot",
    "https://t.me/s/sansalegiatot",
];

/// Quét tự động các kênh chia sẻ deal Shopee công khai để tìm kiếm và nhặt chữ ký số Facebook/Instagram Reels
pub async fn harvest_social_params_once(
    http: &Client,
    db: &AppDb,
    config: &Config,
) -> Option<String> {
    let Ok(conn) = db.conn() else {
        return None;
    };

    let current_saved = Repository::get_setting(&conn, "shopee_social_params")
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    let mut candidate_links: Vec<String> = Vec::new();
    let mut seen_urls: HashSet<String> = HashSet::new();

    // 1. Quét HTML các kênh công khai để trích xuất link Shopee ngắn
    for &channel_url in DEFAULT_CHANNELS {
        let resp = match http
            .get(channel_url)
            .header("User-Agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.0 Mobile/15E148 Safari/604.1")
            .timeout(Duration::from_secs(8))
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        if !resp.status().is_success() {
            continue;
        }

        let html = match resp.text().await {
            Ok(t) => t,
            Err(_) => continue,
        };

        // Tìm các link s.shopee.vn
        let mut start_idx = 0;
        while let Some(pos) = html[start_idx..].find("s.shopee.vn/") {
            let actual_pos = start_idx + pos;
            let end_pos = html[actual_pos..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>' || c == ')' || c == ']')
                .map(|p| actual_pos + p)
                .unwrap_or_else(|| html.len());

            let raw_link = &html[actual_pos..end_pos];
            let full_url = if !raw_link.starts_with("http") {
                format!("https://{}", raw_link)
            } else {
                raw_link.to_string()
            };

            if seen_urls.insert(full_url.clone()) {
                candidate_links.push(full_url);
                if candidate_links.len() >= 25 {
                    break;
                }
            }

            start_idx = end_pos;
            if start_idx >= html.len() {
                break;
            }
        }

        if candidate_links.len() >= 25 {
            break;
        }
    }

    if candidate_links.is_empty() {
        return None;
    }

    // 2. Thử truy vấn các link để tìm kiếm chuỗi tham số Facebook / Instagram
    for link in candidate_links {
        // Thử trích xuất ngay nếu link đã chứa query parameters
        if let Some(params) = extract_social_passthrough_params(&link) {
            if params != current_saved {
                let _ = Repository::set_setting(&conn, "shopee_social_params", &params).await;
                info!("🎉 [SOCIAL_HARVESTER] Tự động nhặt được bộ tham số Shopee FB/IG mới từ link nguồn: {}", params);
                notify_admin_harvested(&params, config).await;
                return Some(params);
            }
        }

        // Nếu link ngắn, gửi HEAD/GET để lần theo URL chuyển hướng (Redirect Location)
        let probe_resp = match http
            .get(&link)
            .header("User-Agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.0 Mobile/15E148 Safari/604.1")
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        let final_url = probe_resp.url().as_str();
        if let Some(params) = extract_social_passthrough_params(final_url) {
            if params != current_saved {
                let _ = Repository::set_setting(&conn, "shopee_social_params", &params).await;
                info!("🎉 [SOCIAL_HARVESTER] Tự động nhặt được bộ tham số Shopee FB/IG mới sau redirect: {}", params);
                notify_admin_harvested(&params, config).await;
                return Some(params);
            }
        }
    }

    None
}

async fn notify_admin_harvested(params: &str, config: &Config) {
    let admin_id = config.admin_telegram_id;
    let bot_token = &config.telegram_token;
    if bot_token.contains("DEMO_TOKEN") || bot_token.trim().is_empty() {
        return;
    }
    let Some(chat_id) = admin_id else {
        return;
    };

    let bot = Bot::new(bot_token);
    let msg = format!(
        "🤖 <b>[TỰ ĐỘNG CẬP NHẬT CHỮ KÝ SHOPEE FB/INSTA]</b>\n\n\
         Hệ thống vừa tự động quét được bộ tham số phiên mạng xã hội mới:\n\
         <code>{}</code>\n\n\
         ✅ Mọi link Shopee cấp cho khách từ thời điểm này sẽ tự động gắn chữ ký này để mở khóa <b>Mã Facebook / Instagram 20% - 30%</b>!",
        escape_html(params)
    );

    let _ = bot
        .send_message(ChatId(chat_id), msg)
        .parse_mode(ParseMode::Html)
        .await;
}

/// Khởi chạy tác vụ nền tự động đi nhặt tham số Shopee Social định kỳ mỗi 30 phút
pub fn spawn_social_harvester_task(db: AppDb, config: Config) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await;

        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap_or_else(|_| Client::new());

        let mut ticker = tokio::time::interval(Duration::from_secs(1800)); // Quét mỗi 30 phút
        loop {
            ticker.tick().await;
            if let Some(harvested) = harvest_social_params_once(&http, &db, &config).await {
                info!("✅ [SOCIAL_HARVESTER] Đã đồng bộ chữ ký Shopee Social mới: {}", harvested);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_social_from_sample() {
        let sample = "https://s.shopee.vn/test?sm_sig_id=SIG_SAMPLE_123&channel_type=ig&content_type=REELS";
        let res = extract_social_passthrough_params(sample);
        assert!(res.is_some());
        let val = res.unwrap();
        assert!(val.contains("sm_sig_id=SIG_SAMPLE_123"));
        assert!(val.contains("channel_type=ig"));
    }
}

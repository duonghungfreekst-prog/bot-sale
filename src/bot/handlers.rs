use std::sync::Arc;
use chrono::Utc;
use teloxide::{
    prelude::*,
    types::{InputFile, LinkPreviewOptions, ParseMode},
    utils::command::BotCommands,
};
use tracing::{error, warn};

use super::keyboards::{
    batch_action_keyboard, deal_action_keyboard, main_menu_keyboard, persistent_reply_keyboard,
};
use crate::config::Config;
use crate::db::models::ClickRecord;
use crate::db::repository::Repository;
use crate::db::AppDb;
use crate::services::deal_parser::{AffiliateSettings, DealParser, ParsedDeal};
use crate::utils::{escape_html, format_vnd};

#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase", description = "Danh sách lệnh hỗ trợ:")]
pub enum Command {
    #[command(description = "Bắt đầu / Dán link sản phẩm nhận ưu đãi")]
    Start,
    #[command(description = "Xem hướng dẫn chi tiết cách áp mã giảm giá")]
    Help,
    #[command(description = "Chia sẻ Thế Giới Siêu Sale cho bạn bè cùng săn sale")]
    Share,
    #[command(description = "Xem thống kê hoạt động bot (Dành cho Admin)")]
    Stats,
}

/// Đọc Telegram ID của Quản trị viên từ DB (Web Admin) ưu tiên trước biến môi trường
pub(crate) async fn resolve_admin_telegram_id(conn: &libsql::Connection, config: &Config) -> Option<i64> {
    Repository::get_setting(conn, "admin_telegram_id")
        .await
        .ok()
        .flatten()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&id| id != 0)
        .or(config.admin_telegram_id)
}

/// Tùy chọn tắt khung xem trước liên kết cồng kềnh trên Telegram
fn disabled_link_preview() -> LinkPreviewOptions {
    LinkPreviewOptions {
        is_disabled: true,
        url: None,
        prefer_small_media: false,
        prefer_large_media: false,
        show_above_text: false,
    }
}

fn platform_matches(deal_platform: &str, parsed_platform: &str) -> bool {
    deal_platform.trim().replace(' ', "").eq_ignore_ascii_case(&parsed_platform.trim().replace(' ', ""))
}

/// Chỉ tự động probe chữ ký FB/IG cho Admin khi tin nhắn có tham số/cầu nối tiềm năng (tránh làm chậm link sản phẩm thường)
fn should_probe_admin_social_input(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("sm_sig_id=")
        || lower.contains("encrypted_payload=")
        || lower.contains("fb_content_id=")
        || lower.contains("channel_type=")
    {
        return true;
    }
    if lower.contains("shopee.vn/product/")
        || lower.contains("shopee.vn/share-video/")
        || lower.contains("vn.shp.ee/")
        || lower.contains("shp.ee/")
        || lower.contains("tiktok.com")
        || lower.contains("tiki.vn")
        || lower.contains("ti.ki")
        || lower.contains("lazada.vn")
        || lower.contains("lzd.co")
        || lower.contains("alibaba.com")
    {
        return false;
    }
    true
}

/// Định dạng hiển thị link gọn gàng trên Telegram (tránh URL trực tiếp dài làm rối khung chat)
pub fn format_telegram_link_display(url: &str, platform: &str, is_wrapped: bool) -> String {
    let trimmed = url.trim();
    if is_wrapped || trimmed.chars().count() <= 75 {
        escape_html(trimmed)
    } else {
        format!(
            "<a href=\"{}\">🔗 Bấm mở {} kèm ưu đãi ↗</a>",
            escape_html(trimmed),
            escape_html(platform)
        )
    }
}

/// Cấu hình Affiliate nạp từ DB (Web Admin) ưu tiên trước biến môi trường
pub(crate) struct OwnedAffSettings {
    pub deeplink_template: String,
    pub accesstrade_token: Option<String>,
    pub campaign_shopee: Option<String>,
    pub campaign_lazada: Option<String>,
    pub campaign_tiki: Option<String>,
    pub campaign_tiktok: Option<String>,
    pub shopee_affiliate_id: Option<String>,
    pub shopee_app_id: Option<String>,
    pub shopee_app_secret: Option<String>,
    pub shopee_social_params: Option<String>,
    pub shortlink_base: Option<String>,
}

impl OwnedAffSettings {
    pub(crate) fn as_borrowed(&self) -> AffiliateSettings<'_> {
        AffiliateSettings {
            deeplink_template: &self.deeplink_template,
            accesstrade_token: self.accesstrade_token.as_deref(),
            campaign_shopee: self.campaign_shopee.as_deref(),
            campaign_lazada: self.campaign_lazada.as_deref(),
            campaign_tiki: self.campaign_tiki.as_deref(),
            campaign_tiktok: self.campaign_tiktok.as_deref(),
            shopee_affiliate_id: self.shopee_affiliate_id.as_deref(),
            shopee_app_id: self.shopee_app_id.as_deref(),
            shopee_app_secret: self.shopee_app_secret.as_deref(),
            shopee_social_params: self.shopee_social_params.as_deref(),
        }
    }
}

pub(crate) async fn load_owned_aff_settings(
    conn: &libsql::Connection,
    config: &Config,
) -> OwnedAffSettings {
    let read_opt = |k: &'static str| async move {
        Repository::get_setting(conn, k)
            .await
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
    };

    let deeplink_template = read_opt("affiliate_template")
        .await
        .unwrap_or_else(|| config.accesstrade_deeplink_template.clone());
    let accesstrade_token = read_opt("accesstrade_api_token")
        .await
        .or_else(|| config.accesstrade_api_token.clone());
    let campaign_shopee = read_opt("campaign_shopee").await;
    let campaign_lazada = read_opt("campaign_lazada").await;
    let campaign_tiki = read_opt("campaign_tiki").await;
    let campaign_tiktok = read_opt("campaign_tiktok").await;
    let shopee_affiliate_id = read_opt("shopee_affiliate_id")
        .await
        .or_else(|| config.shopee_affiliate_id.clone());
    let shopee_app_id = read_opt("shopee_app_id")
        .await
        .or_else(|| config.shopee_app_id.clone());
    let shopee_app_secret = read_opt("shopee_app_secret")
        .await
        .or_else(|| config.shopee_app_secret.clone());
    let shopee_social_params = read_opt("shopee_social_params")
        .await
        .or_else(|| config.shopee_social_params.clone());
    let shortlink_db = read_opt("shortlink_base_url").await;
    let tunnel_snap = crate::services::tunnel::wait_for_tunnel_if_starting(
        config,
        std::time::Duration::from_secs(10),
    )
    .await;
    let shortlink_base = crate::config::resolve_pc_runtime_shortlink_base_url(
        shortlink_db.as_deref(),
        config.shortlink_base_url.as_deref(),
        config.public_base_url.as_deref(),
        tunnel_snap.quick_tunnel_url.as_deref(),
        tunnel_snap.has_named_token,
    );

    OwnedAffSettings {
        deeplink_template,
        accesstrade_token,
        campaign_shopee,
        campaign_lazada,
        campaign_tiki,
        campaign_tiktok,
        shopee_affiliate_id,
        shopee_app_id,
        shopee_app_secret,
        shopee_social_params,
        shortlink_base,
    }
}

/// Xử lý tin nhắn văn bản hoặc bài đăng kèm Ảnh/Video có chứa link
pub async fn handle_message(
    bot: Bot,
    msg: Message,
    db: AppDb,
    config: Config,
    parser: Arc<DealParser>,
) -> ResponseResult<()> {
    let text = match msg.text().or_else(|| msg.caption()) {
        Some(t) if !t.trim().is_empty() => t.trim(),
        _ => return Ok(()),
    };

    let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
    let username = msg.from.as_ref().and_then(|u| u.username.as_deref());
    let full_name = msg
        .from
        .as_ref()
        .map(|u| u.full_name())
        .unwrap_or_else(|| "Bạn".to_string());

    // Xử lý các nút bấm từ Reply Keyboard thường trực hoặc từ khóa nhanh
    let lower_text = text.to_lowercase();
    let mapped_cmd = match text {
        "🚀 Bắt Đầu" | "Bắt Đầu" | "start" => Some(Command::Start),
        "📖 Hướng Dẫn Săn Sale" | "Hướng Dẫn" | "help" => Some(Command::Help),
        "🚀 Chia Sẻ Cho Bạn Bè" | "Chia Sẻ" | "share" => Some(Command::Share),
        _ => None,
    };

    if let Some(cmd) = mapped_cmd {
        return handle_command(bot, msg, cmd, db, config, parser).await;
    }

    // Nếu người dùng bấm nút Kho Mã cũ hoặc gõ /deals /ma -> nhắc dán link sản phẩm trực tiếp
    if text == "🎟️ Mã Hot Hôm Nay"
        || text == "🎟️ Kho Mã 20% - 35%+"
        || text == "🎟️ Kho Mã 20% - 35%"
        || text == "Kho Mã 20% - 35%"
        || lower_text == "deals"
        || lower_text == "ma"
        || lower_text.starts_with("/deals")
        || lower_text.starts_with("/ma")
    {
        bot.send_message(
            msg.chat.id,
            "👉 Hãy <b>dán trực tiếp link sản phẩm Shopee, Lazada, Tiki hoặc TikTok Shop</b> vào đây để bot tạo link mua kèm mã khớp riêng cho đúng sản phẩm đó nhé!",
        )
        .parse_mode(ParseMode::Html)
        .reply_markup(persistent_reply_keyboard())
        .await?;
        return Ok(());
    }

    // Nếu tin nhắn bắt đầu bằng lệnh '/'
    if text.starts_with('/') {
        if let Ok(cmd) = Command::parse(text, &config.bot_username) {
            return handle_command(bot, msg, cmd, db, config, parser).await;
        }
    }

    // Kiểm tra xem tin nhắn có chứa 1 hay nhiều URL link sản phẩm không (hỗ trợ cả caption ảnh, tối đa 5 link/tin & mọi sàn đã duyệt trên ACCESSTRADE)
    if parser.extract_first_url(text).is_some() {
        let conn = match db.conn() {
            Ok(c) => c,
            Err(e) => {
                error!("Lỗi kết nối DB: {}", e);
                return Ok(());
            }
        };

        let (user, _) = match Repository::get_or_create_user(
            &conn,
            user_id,
            username,
            &full_name,
            None,
        )
        .await
        {
            Ok(u) => u,
            Err(e) => {
                error!("Lỗi lấy hoặc tạo user: {}", e);
                return Ok(());
            }
        };

        let admin_id_opt = resolve_admin_telegram_id(&conn, &config).await;
        let is_admin = admin_id_opt.map(|id| id == user_id).unwrap_or(false);
        if is_admin && should_probe_admin_social_input(text) {
            let http_probe = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .redirect(reqwest::redirect::Policy::limited(5))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            if let Some(social_params) =
                crate::services::social_harvester::resolve_social_params_from_input(&http_probe, text).await
            {
                if crate::services::shopee_affiliate::social_param_strength(&social_params) >= 3 {
                    let _ = Repository::set_setting(&conn, "shopee_social_params", &social_params).await;
                    let _ = bot
                        .send_message(
                            msg.chat.id,
                            format!(
                                "⚡ <b>[QUẢN TRỊ VIÊN]</b> Đã nạp chữ ký Shopee FB/Insta Reels:\n<code>{}</code>\n\n✅ Mọi link Shopee từ giờ sẽ gắn chữ ký này!",
                                escape_html(&social_params)
                            ),
                        )
                        .parse_mode(ParseMode::Html)
                        .await;
                }
            }
        }

        let owned_aff = load_owned_aff_settings(&conn, &config).await;
        let is_wrapped = owned_aff.shortlink_base.is_some();
        let approved_dynamic = parser
            .accesstrade()
            .get_or_load_approved_campaigns(owned_aff.accesstrade_token.as_deref())
            .await;
        let extracted_urls =
            parser.extract_all_urls_with_approved(text, 5, approved_dynamic.as_ref());
        if !extracted_urls.is_empty() {
            if extracted_urls.len() >= 2 {
            let wait_label = if is_wrapped {
                format!("🔍 <i>Đang phân tích và bọc hàng loạt {} link sản phẩm cho bạn...</i>", extracted_urls.len())
            } else {
                format!("🔍 <i>Đang phân tích và tạo hàng loạt {} link ưu đãi cho bạn...</i>", extracted_urls.len())
            };
            let wait_msg = bot
                .send_message(msg.chat.id, wait_label)
                .parse_mode(ParseMode::Html)
                .await?;

            let active_deals = Repository::get_active_deals(&conn, None)
                .await
                .unwrap_or_default();
            let mut summary_blocks = Vec::new();
            let mut btn_items = Vec::new();
            let mut rewritten_post = text.to_string();
            let mut text_without_urls = text.to_string();

            for (idx, raw_url) in extracted_urls.iter().enumerate() {
                text_without_urls = text_without_urls.replace(raw_url, " ");
                match parser
                    .process_link(raw_url, user.telegram_id, None, owned_aff.as_borrowed())
                    .await
                {
                    Ok(parsed) => {
                        let click_record = ClickRecord {
                            id: parsed.click_id.clone(),
                            user_id: user.telegram_id,
                            platform: parsed.platform.as_str().to_string(),
                            original_url: parsed.original_url.clone(),
                            clean_url: parsed.clean_url.clone(),
                            affiliate_url: parsed.affiliate_url.clone(),
                            sub_id: parsed.sub_id.clone(),
                            created_at: Utc::now().to_rfc3339(),
                        };
                        if let Err(err) = Repository::record_click(&conn, &click_record).await {
                            warn!("Không thể ghi log click batch vào LibSQL: {}", err);
                        }

                        let wrapped_url = Repository::wrap_url_if_public(
                            &conn,
                            owned_aff.shortlink_base.as_deref(),
                            &parsed.affiliate_url,
                            &parsed.product_title,
                            parsed.platform.as_str(),
                        )
                        .await;
                        rewritten_post = rewritten_post.replace(raw_url, &wrapped_url);

                        let price_tag = parsed
                            .product_price_vnd
                            .filter(|&p| p > 0.0)
                            .map(|p| format!(" · {}", format_vnd(p)))
                            .unwrap_or_default();

                        let top_deals = select_top_relevant_deals(&active_deals, &parsed, 1);
                        let mut voucher_hint = top_deals
                            .first()
                            .map(|d| {
                                let code = d
                                    .coupon_code
                                    .as_deref()
                                    .filter(|c| !c.trim().is_empty())
                                    .map(|c| format!(" (Mã: <code>{}</code>)", escape_html(c)))
                                    .unwrap_or_default();
                                format!("\n   👑 {}{}", escape_html(&d.title), code)
                            })
                            .unwrap_or_default();

                        if voucher_hint.is_empty() {
                            let matched_mv = parser
                                .accesstrade()
                                .search_matching_vouchers_for_deal(
                                    parsed.platform.as_str(),
                                    &parsed.product_title,
                                    &parsed.clean_url,
                                    parsed.product_shop.as_deref(),
                                    parsed.product_brand.as_deref(),
                                    parsed.product_price_vnd,
                                    1,
                                )
                                .await;
                            let allow_market_generic = match parsed.platform {
                                crate::services::deal_parser::Platform::Shopee => {
                                    parsed.is_affiliate
                                        || approved_dynamic
                                            .as_ref()
                                            .and_then(|a| a.shopee.as_ref())
                                            .is_some()
                                }
                                _ => true,
                            };
                            if let Some(mv) = matched_mv.into_iter().find(|mv| allow_market_generic || mv.is_shop_specific()) {
                                voucher_hint = format!(
                                    "\n   🎟️ <b>{}</b> · Mã: <code>{}</code>",
                                    escape_html(&mv.discount_badge()),
                                    escape_html(&mv.coupon_code)
                                );
                            }
                        }

                        let link_display = format_telegram_link_display(
                            &wrapped_url,
                            parsed.platform.as_str(),
                            is_wrapped,
                        );
                        summary_blocks.push(format!(
                            "{}. <b>[{}] {}</b>{}\n   👉 {}{}",
                            idx + 1,
                            escape_html(parsed.platform.as_str()),
                            escape_html(&parsed.product_title),
                            escape_html(&price_tag),
                            link_display,
                            voucher_hint
                        ));
                        btn_items.push((
                            format!("🛒 Mở SP #{} ({})", idx + 1, parsed.platform.as_str()),
                            wrapped_url,
                        ));
                    }
                    Err(err) => {
                        summary_blocks.push(format!(
                            "{}. ⚠️ Không phân tích được link ({})",
                            idx + 1,
                            escape_html(&err.to_string())
                        ));
                    }
                }
            }

            let batch_title = if is_wrapped {
                format!("⚡ <b>ĐÃ BỌC HÀNG LOẠT {} LINK SẢN PHẨM</b>", extracted_urls.len())
            } else {
                format!("⚡ <b>ĐÃ TẠO HÀNG LOẠT {} LINK ƯU ĐÃI</b>", extracted_urls.len())
            };
            let copy_title = if is_wrapped {
                "📋 <b>BÀI ĐÃ THAY SẠCH LINK BỌC (Copy đăng kênh/nhóm):</b>"
            } else {
                "📋 <b>BÀI ĐÃ GẮN LINK ƯU ĐÃI (Copy đăng kênh/nhóm):</b>"
            };
            let extra_copy_block = if text_without_urls.trim().chars().count() >= 12 {
                let preview_copy: String = rewritten_post.chars().take(1200).collect();
                format!(
                    "\n\n{}\n<pre>{}</pre>",
                    copy_title,
                    escape_html(&preview_copy)
                )
            } else {
                String::new()
            };

            let mut reply_html = format!(
                "{}\n\n{}{}",
                batch_title,
                summary_blocks.join("\n\n"),
                extra_copy_block
            );
            if reply_html.chars().count() > 3800 {
                reply_html = format!(
                    "{}\n\n{}",
                    batch_title,
                    summary_blocks.join("\n\n")
                );
            }

            let _ = bot.delete_message(msg.chat.id, wait_msg.id).await;
            bot.send_message(msg.chat.id, reply_html)
                .parse_mode(ParseMode::Html)
                .link_preview_options(disabled_link_preview())
                .reply_markup(batch_action_keyboard(&btn_items))
                .await?;
            return Ok(());
        }

        // Gửi thông báo đang tìm và gắn mã giảm giá cho 1 sản phẩm
        let wait_msg = bot
            .send_message(msg.chat.id, "🔍 <i>Đang quét và gắn mã giảm giá tốt nhất cho sản phẩm này...</i>")
            .parse_mode(ParseMode::Html)
            .await?;

        let shortlink_base = owned_aff.shortlink_base.clone();
        let aff_settings = owned_aff.as_borrowed();

        // Bóc tách link và gọi API tạo link thực tế
        match parser
            .process_link(
                text,
                user.telegram_id,
                None, // Toàn bộ hoa hồng chiết khấu chảy trực tiếp về cho chủ bot
                aff_settings,
            )
            .await
        {
            Ok(parsed) => {
                if let Some(ref note) = parsed.affiliate_note {
                    warn!("Ghi chú Affiliate cho sàn {}: {}", parsed.platform.as_str(), note);
                }

                // Ghi nhận lượt click vào Database LibSQL
                let click_record = ClickRecord {
                    id: parsed.click_id.clone(),
                    user_id: user.telegram_id,
                    platform: parsed.platform.as_str().to_string(),
                    original_url: parsed.original_url.clone(),
                    clean_url: parsed.clean_url.clone(),
                    affiliate_url: parsed.affiliate_url.clone(),
                    sub_id: parsed.sub_id.clone(),
                    created_at: Utc::now().to_rfc3339(),
                };

                if let Err(err) = Repository::record_click(&conn, &click_record).await {
                    warn!("Không thể ghi log click vào LibSQL: {}", err);
                }

                // Bọc 100% link đầu ra qua shortlink hệ thống (giống godeal.online)
                let wrapped_aff_url = Repository::wrap_url_if_public(
                    &conn,
                    shortlink_base.as_deref(),
                    &parsed.affiliate_url,
                    &parsed.product_title,
                    parsed.platform.as_str(),
                )
                .await;

                let safe_aff_url = format_telegram_link_display(
                    &wrapped_aff_url,
                    parsed.platform.as_str(),
                    is_wrapped,
                );
                let social_strength = owned_aff
                    .shopee_social_params
                    .as_deref()
                    .map(crate::services::shopee_affiliate::social_param_strength)
                    .unwrap_or(0);
                let display_title = if parsed.product_title == "Sản Phẩm Shopee" {
                    "Sản Phẩm Shopee Đang Chọn".to_string()
                } else {
                    parsed.product_title.clone()
                };
                let header_line = if parsed.is_affiliate {
                    let label = if parsed.platform.as_str().eq_ignore_ascii_case("Shopee") && social_strength >= 3 {
                        "🎯 Link FB / IG Reels · Sẵn sàng áp mã 20-50%".to_string()
                    } else if parsed.platform.as_str().eq_ignore_ascii_case("Shopee") {
                        "🎯 Link Ưu Đãi Shopee Đã Kích Hoạt".to_string()
                    } else {
                        format!("🎯 Link Ưu Đãi {} Đã Kích Hoạt", escape_html(parsed.platform.as_str()))
                    };
                    format!("<b>{}</b>\n👉 {}", label, safe_aff_url)
                } else {
                    let admin_hint = if is_admin && parsed.platform.as_str().eq_ignore_ascii_case("Shopee") {
                        "\n⚙️ <i>Admin: Chưa gắn Shopee Affiliate ID</i>"
                    } else {
                        ""
                    };
                    format!(
                        "👉 <b>Link Mở App {}:</b>\n{}{}",
                        escape_html(parsed.platform.as_str()),
                        safe_aff_url,
                        admin_hint
                    )
                };

                let meta_line = {
                    let mut parts = Vec::new();
                    if let Some(shop) = parsed.product_shop.as_deref() {
                        parts.push(format!("🏪 {}", escape_html(shop)));
                    }
                    if let Some(brand) = parsed.product_brand.as_deref() {
                        parts.push(format!("🏷️ {}", escape_html(brand)));
                    }
                    if let Some(price) = parsed.product_price_vnd.filter(|&p| p > 0.0) {
                        parts.push(format!("💰 <b>{}</b>", escape_html(&format_vnd(price))));
                    }
                    if parts.is_empty() {
                        String::new()
                    } else {
                        format!("{}\n", parts.join(" · "))
                    }
                };

                let mut voucher_buttons: Vec<(String, String)> = Vec::new();

                // Chỉ hiển thị gọn tối đa 2 mã khớp thực tế (loại bài hướng dẫn rác & chống trùng URL)
                let mut extra_vouchers_block = match Repository::get_active_deals(&conn, None).await {
                    Ok(deals) => {
                        let ex_list = select_top_relevant_deals(&deals, &parsed, 2);
                        if ex_list.is_empty() {
                            String::new()
                        } else {
                            let mut s = String::from("\n\n🎁 <b>Mã Nổi Bật Áp Dụng:</b>");
                            for d in ex_list {
                                let enriched_deal_url = enrich_deal_url_for_platform(
                                    &d.deal_url,
                                    &d.platform,
                                    owned_aff.shopee_affiliate_id.as_deref(),
                                    owned_aff.shopee_social_params.as_deref(),
                                    &parsed.sub_id,
                                );
                                let wrapped_deal_url = Repository::wrap_url_if_public(
                                    &conn,
                                    shortlink_base.as_deref(),
                                    &enriched_deal_url,
                                    &d.title,
                                    &d.platform,
                                )
                                .await;
                                let short_desc: String = d.discount_desc.chars().take(70).collect();
                                s.push_str(&format!("\n• {}", escape_html(&short_desc)));
                                if voucher_buttons.len() < 2 && !wrapped_deal_url.trim().is_empty() {
                                    let btn_label = match d.coupon_code.as_deref().filter(|c| !c.trim().is_empty()) {
                                        Some(c) if c.starts_with("KOL_IG") => "📸 Mã IG 50%".to_string(),
                                        Some(c) if c.starts_with("KOL_FB") => "📘 Mã FB 30%".to_string(),
                                        Some(c) => format!("🎟️ {}", c),
                                        None => format!("🎟️ {}", d.title.chars().take(18).collect::<String>()),
                                    };
                                    voucher_buttons.push((btn_label, wrapped_deal_url));
                                }
                            }
                            s
                        }
                    }
                    Err(_) => String::new(),
                };

                let approved_cached = parser.accesstrade().peek_approved_campaigns().await;
                let allow_market_generic = match parsed.platform {
                    crate::services::deal_parser::Platform::Shopee => {
                        parsed.is_affiliate || approved_cached.as_ref().and_then(|a| a.shopee.as_ref()).is_some()
                    }
                    _ => true,
                };
                let matched_market: Vec<_> = parser
                    .accesstrade()
                    .search_matching_vouchers_for_deal(
                        parsed.platform.as_str(),
                        &parsed.product_title,
                        &parsed.clean_url,
                        parsed.product_shop.as_deref(),
                        parsed.product_brand.as_deref(),
                        parsed.product_price_vnd,
                        4,
                    )
                    .await
                    .into_iter()
                    .filter(|mv| allow_market_generic || mv.is_shop_specific())
                    .take(2)
                    .collect();
                if !matched_market.is_empty() {
                    extra_vouchers_block.push_str("\n\n🎟️ <b>Mã Khớp Đúng Shop/Hãng:</b>");
                    for mv in matched_market {
                        let direct_voucher_url = parser
                            .accesstrade()
                            .verified_offer_url(&mv, approved_cached.as_ref())
                            .await;
                        let enriched_voucher_url = enrich_deal_url_for_platform(
                            &direct_voucher_url,
                            &mv.platform,
                            owned_aff.shopee_affiliate_id.as_deref(),
                            owned_aff.shopee_social_params.as_deref(),
                            &parsed.sub_id,
                        );
                        let wrapped_voucher_url = Repository::wrap_url_if_public(
                            &conn,
                            shortlink_base.as_deref(),
                            &enriched_voucher_url,
                            &format!("{} - {}", mv.shop_name, mv.discount_desc),
                            &mv.platform,
                        )
                        .await;
                        extra_vouchers_block.push_str(&format!(
                            "\n• <b>[{}]</b> <code>{}</code> — {}",
                            escape_html(&mv.discount_badge()),
                            escape_html(&mv.coupon_code),
                            escape_html(&mv.discount_desc)
                        ));
                        if voucher_buttons.len() < 2 {
                            voucher_buttons.push((
                                format!("🎟️ {} · {}", mv.discount_badge(), mv.coupon_code),
                                wrapped_voucher_url,
                            ));
                        }
                    }
                }

                // Bọc luôn các nút cổng voucher sàn để 100% nút trên Telegram đi qua domain bọc link
                let raw_portals: Vec<(&str, &str)> = match parsed.platform.as_str().to_lowercase().as_str() {
                    "shopee" => vec![
                        ("🎟️ Kho Mã Shopee", "https://shopee.vn/m/ma-giam-gia"),
                        ("🎬 Săn Mã Video", "https://shopee.vn/m/shopee-video"),
                    ],
                    "lazada" => vec![("🎟️ Voucher Lazada", "https://www.lazada.vn/voucher")],
                    "tiki" => vec![("🎟️ Cổng Mã Tiki", "https://tiki.vn/khuyen-mai/ma-giam-gia")],
                    "alibaba" => vec![("🌐 Alibaba.com", "https://www.alibaba.com")],
                    _ => Vec::new(),
                };
                let mut portal_buttons = Vec::with_capacity(raw_portals.len());
                let shopee_direct_aff = owned_aff
                    .shopee_affiliate_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|id| crate::services::shopee_affiliate::is_valid_shopee_affiliate_id(id));
                for (label, raw_portal_url) in raw_portals {
                    let portal_target = if parsed.platform.as_str().eq_ignore_ascii_case("Shopee") {
                        let base_portal = if let Some(aff_id) = shopee_direct_aff {
                            crate::services::shopee_affiliate::build_direct_an_redir_link(
                                raw_portal_url,
                                aff_id,
                                &parsed.sub_id,
                            )
                        } else {
                            raw_portal_url.to_string()
                        };
                        crate::services::shopee_affiliate::append_social_params_to_shopee_link(
                            &base_portal,
                            owned_aff.shopee_social_params.as_deref(),
                        )
                    } else {
                        raw_portal_url.to_string()
                    };
                    let wrapped_portal = Repository::wrap_url_if_public(
                        &conn,
                        shortlink_base.as_deref(),
                        &portal_target,
                        label,
                        parsed.platform.as_str(),
                    )
                    .await;
                    portal_buttons.push((label.to_string(), wrapped_portal));
                }

                let back_line = if parsed.platform.as_str() == "TikTokShop" {
                    String::new()
                } else {
                    format!("\n\n⏰ <b>Back mã:</b> {}", escape_html(&parsed.primary_voucher.back_hours))
                };
                let tip_line = parsed
                    .tips
                    .first()
                    .map(|t| escape_html(t))
                    .unwrap_or_else(|| {
                        "Xoá sản phẩm cũ khỏi giỏ hàng trước khi bấm mua để nhận đúng mã.".to_string()
                    });

                let reply_html = format!(
                    "🛍️ <b>{}</b>\n\
                     {}{}{}{}\n\
                     💡 <i>{}</i>",
                    escape_html(&display_title),
                    meta_line,
                    header_line,
                    extra_vouchers_block,
                    back_line,
                    tip_line,
                );

                // Xóa tin nhắn chờ và gửi kèm ảnh sản phẩm thật (tự fallback sang tin nhắn chữ nếu CDN chặn)
                let _ = bot.delete_message(msg.chat.id, wait_msg.id).await;
                let keyboard = deal_action_keyboard(
                    &wrapped_aff_url,
                    parsed.platform.as_str(),
                    &voucher_buttons,
                    &portal_buttons,
                );
                let mut sent_photo = false;
                if let Some(ref img_url_str) = parsed.product_image_url {
                    if reply_html.chars().count() <= 960 {
                        if let Ok(parsed_img) = url::Url::parse(img_url_str) {
                            if bot
                                .send_photo(msg.chat.id, InputFile::url(parsed_img))
                                .caption(&reply_html)
                                .parse_mode(ParseMode::Html)
                                .reply_markup(keyboard.clone())
                                .await
                                .is_ok()
                            {
                                sent_photo = true;
                            }
                        }
                    }
                }
                if !sent_photo {
                    bot.send_message(msg.chat.id, reply_html)
                        .parse_mode(ParseMode::Html)
                        .link_preview_options(LinkPreviewOptions {
                            is_disabled: true,
                            url: None,
                            prefer_small_media: false,
                            prefer_large_media: false,
                            show_above_text: false,
                        })
                        .reply_markup(keyboard)
                        .await?;
                }
            }
            Err(err) => {
                let _ = bot.delete_message(msg.chat.id, wait_msg.id).await;
                let err_text = format!(
                    "⚠️ <b>Không thể phân tích link này</b>\n\n\
                     Chi tiết: {}\n\n\
                     👉 Vui lòng dán đường link sản phẩm từ <b>Shopee</b>, <b>Lazada</b>, <b>Tiki</b>, <b>TikTok Shop</b>, <b>Alibaba</b> hoặc các sàn đã duyệt trên ACCESSTRADE.",
                    escape_html(&err.to_string())
                );
                bot.send_message(msg.chat.id, err_text)
                    .parse_mode(ParseMode::Html)
                    .await?;
            }
        }
        return Ok(());
        }
    }

    // Tin nhắn thông thường không có link
    let reply = format!(
        "👋 Chào <b>{}</b>!\n\n\
         🤖 <b>Bot Tự Động Săn Mã Giảm Giá Đa Sàn (Shopee, Lazada, Tiki, TikTok Shop, Alibaba)</b>\n\n\
         📌 <b>Cách lấy mã giảm giá cực đơn giản:</b>\n\
         1️⃣ Mở app <b>Shopee, Lazada, Tiki, TikTok Shop hoặc Alibaba</b> và copy link món đồ bạn muốn mua.\n\
         2️⃣ Dán trực tiếp đường link vào khung chat này.\n\
         3️⃣ Bot sẽ tự động kiểm tra ưu đãi và tạo link mua hàng kèm mã giảm giá cho bạn!\n\n\
         👇 <i>Bấm các nút chức năng bên dưới hoặc gửi link sản phẩm ngay:</i>",
        escape_html(&full_name)
    );

    bot.send_message(msg.chat.id, reply)
        .parse_mode(ParseMode::Html)
        .reply_markup(main_menu_keyboard(&config.bot_username))
        .await?;

    Ok(())
}

/// Xử lý các lệnh Slash Commands (/start, /help, /share, /stats)
pub async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    db: AppDb,
    config: Config,
    _parser: Arc<DealParser>,
) -> ResponseResult<()> {
    let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
    let username = msg.from.as_ref().and_then(|u| u.username.as_deref());
    let full_name = msg
        .from
        .as_ref()
        .map(|u| u.full_name())
        .unwrap_or_else(|| "Bạn".to_string());

    let conn = match db.conn() {
        Ok(c) => c,
        Err(e) => {
            error!("Lỗi DB: {}", e);
            bot.send_message(msg.chat.id, "❌ Hệ thống cơ sở dữ liệu tạm thời bận, vui lòng thử lại sau.")
                .await?;
            return Ok(());
        }
    };

    match cmd {
        Command::Start => {
            let _ = Repository::get_or_create_user(
                &conn,
                user_id,
                username,
                &full_name,
                None,
            )
            .await;

            // Bật bàn phím chức năng thường trực dưới khung chat
            let _ = bot.send_message(msg.chat.id, "🔘 <i>Menu săn sale nhanh đã được ghim cố định ở đáy khung chat!</i>")
                .parse_mode(ParseMode::Html)
                .reply_markup(persistent_reply_keyboard())
                .await;

            let welcome = "⚡ <b>CHÀO MỪNG BẠN ĐẾN VỚI THẾ GIỚI SIÊU SALE</b> ⚡\n\n\
                 🔥 <b>Mua hàng tiết kiệm hơn chỉ với 1 bước:</b>\n\
                 Chỉ cần <b>Dán trực tiếp link sản phẩm Shopee, Lazada, Tiki, TikTok Shop, Alibaba hoặc các đối tác chính hãng</b> vào khung chat này, bot sẽ tự phân tích sản phẩm/shop và trả về link mua kèm mã khớp riêng ngay cho bạn!\n\n\
                 👇 <i>Hãy dán thử 1 link sản phẩm bất kỳ vào đây để trải nghiệm ngay:</i>";

            bot.send_message(msg.chat.id, welcome)
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username))
                .await?;
        }

        Command::Help => {
            let help_text = 
                "📖 <b>HƯỚNG DẪN 3 BƯỚC NHẬN LINK &amp; MÃ GIẢM GIÁ:</b>\n\n\
                 1️⃣ <b>Bước 1:</b> Vào App Shopee, Lazada, Tiki hoặc TikTok Shop chọn sản phẩm bạn muốn mua.\n\
                 2️⃣ <b>Bước 2:</b> Bấm nút <b>Chia sẻ</b> -&gt; chọn <b>Sao chép liên kết</b>.\n\
                 3️⃣ <b>Bước 3:</b> Dán đường link vào khung chat bot này. Bot sẽ trả về link mua sản phẩm kèm mã khớp trực tiếp với shop/sản phẩm đó (nếu có) và nút Cổng Voucher sàn.\n\n\
                 ⚠️ <b>MẸO ÁP MÃ THÀNH CÔNG 100%:</b>\n\
                 • Bấm vào link bot gửi để mở App sàn.\n\
                 • Xoá sản phẩm cũ trong giỏ hàng trước khi mua để mã không bị xung đột.\n\
                 • Canh đúng các khung giờ back mã: <code>0H · 9H · 12H · 15H · 20H</code> để áp mã giảm sâu nhất!";

            bot.send_message(msg.chat.id, help_text)
                .parse_mode(ParseMode::Html)
                .await?;
        }

        Command::Share => {
            let share_link = format!("https://t.me/{}", config.bot_username);

            let share_html = format!(
                "⚡ <b>CHIA SẺ THẾ GIỚI SIÊU SALE CHO BẠN BÈ CÙNG SĂN SALE</b> ⚡\n\n\
                 Giúp bạn bè mua sắm tiết kiệm mỗi ngày trên Shopee, Lazada, Tiki &amp; TikTok Shop!\n\n\
                 🔗 <b>Link bot:</b>\n\
                 <code>{}</code>\n\n\
                 👉 <i>Hãy copy link trên và chia sẻ lên Facebook, Zalo hoặc nhóm săn sale nhé!</i>",
                escape_html(&share_link)
            );

            bot.send_message(msg.chat.id, share_html)
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username))
                .await?;
        }

        Command::Stats => {
            let admin_id_opt = resolve_admin_telegram_id(&conn, &config).await;
            let can_view_admin_stats = match admin_id_opt {
                Some(admin_id) => admin_id == user_id,
                None => true, // Chưa gắn Admin ID: cho phép xem lần đầu để lấy ID gắn vào Web Admin
            };

            let stats_html = if can_view_admin_stats {
                let stats = Repository::get_admin_dashboard_stats(&conn).await.unwrap_or_default();
                let tracked_orders = Repository::get_recent_tracked_orders(&conn, 100)
                    .await
                    .unwrap_or_default();

                format!(
                    "📊 <b>BÁO CÁO HOẠT ĐỘNG BOT (DÀNH CHO CHỦ BOT)</b>\n\n\
                     🆔 <b>Telegram ID của bạn:</b> <code>{}</code> <i>(Dùng để nhận báo đơn trên http://127.0.0.1:{}/admin)</i>\n\
                     👥 <b>Tổng số người dùng bot:</b> <b>{} người</b>\n\
                     🔗 <b>Tổng số lượt dán link &amp; tạo mã:</b> <b>{} lượt</b>\n\
                     🧾 <b>Số đơn hàng đã đối soát gần đây:</b> <b>{} đơn</b>\n\n\
                     💰 <b>Hoa hồng tiếp thị:</b>\n\
                     100% doanh thu chiết khấu từ các lượt click trên sẽ được sàn TMĐT đối soát về tài khoản Affiliate của bạn!",
                    user_id, config.port, stats.total_users, stats.total_clicks, tracked_orders.len()
                )
            } else {
                format!(
                    "🆔 <b>Telegram ID của bạn:</b> <code>{}</code>\n\n\
                     👉 Hãy dán trực tiếp link sản phẩm Shopee, Lazada, Tiki hoặc TikTok Shop vào đây để săn mã giảm giá nhé!",
                    user_id
                )
            };

            bot.send_message(msg.chat.id, stats_html)
                .parse_mode(ParseMode::Html)
                .await?;
        }
    }

    Ok(())
}

/// Xử lý các sự kiện bấm nút Inline Keyboard
pub async fn handle_callback(
    bot: Bot,
    q: CallbackQuery,
    _db: AppDb,
    config: Config,
    _parser: Arc<DealParser>,
) -> ResponseResult<()> {
    let data = match q.data.as_deref() {
        Some(d) => d,
        None => return Ok(()),
    };

    let chat_id = match q.message {
        Some(ref msg) => msg.chat().id,
        None => return Ok(()),
    };

    let _ = bot.answer_callback_query(q.id).await;

    if data.starts_with("deals_page:") || data.starts_with("deals_refresh:") {
        bot.send_message(
            chat_id,
            "👉 Hãy <b>dán trực tiếp link sản phẩm</b> vào chat để nhận link kèm mã khớp riêng cho sản phẩm đó!",
        )
        .parse_mode(ParseMode::Html)
        .await?;
        return Ok(());
    }

    match data {
        "menu_guide" => {
            let guide = "📖 <b>HƯỚNG DẪN 3 BƯỚC NHẬN MÃ GIẢM GIÁ:</b>\n\n\
                         1️⃣ Copy link sản phẩm trên app Shopee, Lazada, Tiki hoặc TikTok Shop.\n\
                         2️⃣ Dán link vào khung chat này để bot tạo link mua kèm mã khớp riêng (nếu có).\n\
                         3️⃣ Bấm nút mở sản phẩm hoặc nút lấy mã đi kèm để đặt hàng!\n\n\
                         <i>Mẹo: Xóa sản phẩm cũ trong giỏ hàng trước khi mua để tránh bị lỗi mã!</i>";
            bot.send_message(chat_id, guide)
                .parse_mode(ParseMode::Html)
                .await?;
        }
        "menu_main" => {
            bot.send_message(chat_id, "🏠 <b>MENU CHÍNH THẾ GIỚI SIÊU SALE</b>")
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username))
                .await?;
        }
        _ => {}
    }

    Ok(())
}

/// Gắn Shopee Affiliate ID (`an_redir`) & chữ ký FB/IG Reels cho link mã ưu đãi Shopee (giữ nguyên link sạch nếu chưa cấu hình).
pub(crate) fn enrich_deal_url_for_platform(
    raw_url: &str,
    platform: &str,
    shopee_aff_id: Option<&str>,
    shopee_social_params: Option<&str>,
    sub_id: &str,
) -> String {
    if !platform.trim().eq_ignore_ascii_case("Shopee") {
        return raw_url.to_string();
    }
    let trimmed = raw_url.trim();
    let lower = trimmed.to_ascii_lowercase();
    let base_url = if let Some(aff_id) = shopee_aff_id
        .map(str::trim)
        .filter(|id| crate::services::shopee_affiliate::is_valid_shopee_affiliate_id(id))
    {
        if lower.contains("s.shopee.vn/an_redir") || lower.contains("mmp_pid=an_") {
            trimmed.to_string()
        } else {
            crate::services::shopee_affiliate::build_direct_an_redir_link(trimmed, aff_id, sub_id)
        }
    } else {
        trimmed.to_string()
    };
    crate::services::shopee_affiliate::append_social_params_to_shopee_link(&base_url, shopee_social_params)
}

fn parse_discount_pct_hint(text: &str) -> u32 {
    let bytes = text.as_bytes();
    let mut best = 0u32;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'%' && i > 0 {
            let start = bytes[..i]
                .iter()
                .rposition(|c| !c.is_ascii_digit())
                .map(|p| p + 1)
                .unwrap_or(0);
            if let Ok(v) = text[start..i].parse::<u32>() {
                if v <= 99 {
                    best = best.max(v);
                }
            }
        }
    }
    best
}

/// Kiểm tra mã độc quyền trong DB có thực sự áp được cho sản phẩm đang dán (toàn sàn hoặc khớp shop/từ khóa) hay không
pub fn is_exclusive_deal_relevant(deal: &crate::db::models::DealItem, parsed: &ParsedDeal) -> bool {
    if !platform_matches(&deal.platform, parsed.platform.as_str()) {
        return false;
    }

    // Loại các bài viết hướng dẫn chung không có link săn mã trực tiếp
    let desc_lower = deal.discount_desc.to_lowercase();
    let url_lower = deal.deal_url.to_lowercase();
    let has_direct_voucher_target = url_lower.contains("evcode=")
        || url_lower.contains("promotionid=")
        || url_lower.contains("shopee.vn/search?");
    if !has_direct_voucher_target
        && (desc_lower.contains("hướng dẫn lấy mã")
            || desc_lower.contains("khi ấn link lấy mã")
            || desc_lower.contains("cách lấy mã"))
    {
        return false;
    }

    // Kiểm tra điều kiện giá tối thiểu nếu đã đọc được giá sản phẩm thật
    if let Some(price) = parsed.product_price_vnd.filter(|&p| p > 0.0) {
        if deal.min_spend > 0.0 && price < deal.min_spend {
            return false;
        }
    }

    // Nếu tiêu đề mã có tiền tố [Tên Shop], kiểm tra xem có phải mã toàn sàn hay khớp shop/brand/tên sản phẩm không
    if let Some(rest) = deal.title.trim().strip_prefix('[') {
        if let Some(end_idx) = rest.find(']') {
            let tag = rest[..end_idx].trim().to_lowercase();
            let plat = parsed.platform.as_str().to_lowercase();
            let is_global_tag = tag.is_empty()
                || tag == plat
                || tag.contains("toàn sàn")
                || tag.contains("chung");
            if is_global_tag {
                return true;
            }
            let shop_match = parsed
                .product_shop
                .as_deref()
                .map(|s| s.to_lowercase().contains(&tag) || tag.contains(&s.to_lowercase()))
                .unwrap_or(false);
            let brand_match = parsed
                .product_brand
                .as_deref()
                .map(|b| b.to_lowercase().contains(&tag) || tag.contains(&b.to_lowercase()))
                .unwrap_or(false);
            let title_match = parsed.product_title.to_lowercase().contains(&tag);
            return shop_match || brand_match || title_match;
        }
    }

    true
}

/// Chọn tối đa `limit` mã độc quyền / FB-IG chất lượng nhất (ưu tiên link săn mã thật, % giảm cao, không trùng URL).
pub(crate) fn select_top_relevant_deals<'a>(
    deals: &'a [crate::db::models::DealItem],
    parsed: &ParsedDeal,
    limit: usize,
) -> Vec<&'a crate::db::models::DealItem> {
    let mut ranked: Vec<(u8, bool, u32, i64, &'a crate::db::models::DealItem)> = deals
        .iter()
        .filter(|d| d.is_active && is_exclusive_deal_relevant(d, parsed))
        .map(|d| {
            let url_lower = d.deal_url.to_lowercase();
            let has_voucher_query = url_lower.contains("evcode=")
                || url_lower.contains("promotionid=")
                || url_lower.contains("shopee.vn/search?");
            let pct = parse_discount_pct_hint(&d.discount_desc)
                .max(parse_discount_pct_hint(&d.title));
            let specificity = if d.title.starts_with('[') && !d.title.to_lowercase().contains("toàn sàn") {
                2u8
            } else {
                1u8
            };
            (specificity, has_voucher_query, pct, d.id, d)
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| b.3.cmp(&a.3))
    });

    let mut seen_urls = std::collections::HashSet::new();
    let mut selected = Vec::with_capacity(limit);
    for (_, _, _, _, deal) in ranked {
        let norm_url = deal.deal_url.trim().trim_end_matches('/').to_string();
        if !norm_url.is_empty() && !seen_urls.insert(norm_url) {
            continue;
        }
        selected.push(deal);
        if selected.len() >= limit {
            break;
        }
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::DealItem;
    use crate::services::deal_parser::{Platform, SaleVoucher};

    #[test]
    fn test_is_exclusive_deal_relevant_filters_other_shop_tags() {
        let parsed = ParsedDeal {
            platform: Platform::Tiki,
            product_title: "Sữa bột Enfagrow Enspire 3 850g".to_string(),
            product_price_vnd: Some(822000.0),
            product_shop: Some("Tiki Trading".to_string()),
            product_brand: Some("Enfa".to_string()),
            product_image_url: None,
            original_url: "https://tiki.vn/product-p274504811.html".to_string(),
            resolved_url: "https://tiki.vn/product-p274504811.html".to_string(),
            clean_url: "https://tiki.vn/product-p274504811.html".to_string(),
            affiliate_url: "https://tiki.vn/product-p274504811.html".to_string(),
            is_affiliate: false,
            affiliate_note: None,
            click_id: "c1".to_string(),
            sub_id: "s1".to_string(),
            primary_voucher: SaleVoucher {
                name: "Test".to_string(),
                discount_desc: "Desc".to_string(),
                min_spend_desc: "0đ".to_string(),
                remaining_percent: 80,
                back_hours: "12H".to_string(),
            },
            estimated_cashback_text: String::new(),
            tips: vec![],
        };

        let make_deal = |title: &str| DealItem {
            id: 1,
            platform: "Tiki".to_string(),
            title: title.to_string(),
            discount_desc: "Giảm 50K".to_string(),
            coupon_code: Some("CODE50".to_string()),
            min_spend: 0.0,
            deal_url: "https://tiki.vn".to_string(),
            is_exclusive: true,
            is_active: true,
            expire_at: None,
        };

        assert!(is_exclusive_deal_relevant(&make_deal("[Toàn sàn] Giảm 50K"), &parsed));
        assert!(is_exclusive_deal_relevant(&make_deal("[Tiki Trading] Giảm 45K"), &parsed));
        assert!(!is_exclusive_deal_relevant(&make_deal("[DONA Thời Trang] Giảm 11%"), &parsed));
    }

    #[test]
    fn test_format_telegram_link_display_short_vs_long_direct() {
        let short_wrapped = "https://go.example.com/Ab12Cd34";
        assert_eq!(
            format_telegram_link_display(short_wrapped, "Shopee", true),
            short_wrapped
        );

        let short_direct = "https://s.shopee.vn/2gCo1LuaEU";
        assert_eq!(
            format_telegram_link_display(short_direct, "Shopee", false),
            short_direct
        );

        let long_direct = "https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fproduct%2F123456%2F987654321&affiliate_id=17375520550&sub_id=u1_r0_abc";
        let formatted = format_telegram_link_display(long_direct, "Shopee", false);
        assert!(formatted.starts_with("<a href=\"https://s.shopee.vn/an_redir?"));
        assert!(formatted.contains("Bấm mở Shopee kèm ưu đãi"));
    }

    #[test]
    fn test_should_probe_admin_social_input_and_platform_match() {
        assert!(!should_probe_admin_social_input("https://vn.shp.ee/oSMPdPRU"));
        assert!(!should_probe_admin_social_input("https://shopee.vn/product/123/456"));
        assert!(!should_probe_admin_social_input("https://vt.tiktok.com/ZSYabc/"));
        assert!(should_probe_admin_social_input("https://godeal.online/WdIhcsBB"));
        assert!(should_probe_admin_social_input("https://s.shopee.vn/6L6Hj9wivc?sm_sig_id=SIG1"));

        assert!(platform_matches("TikTok Shop", "TikTokShop"));
        assert!(platform_matches("shopee", "Shopee"));
        assert!(!platform_matches("Shopee", "TikTokShop"));
    }

    #[test]
    fn test_select_top_relevant_deals_filters_guides_and_dedups_urls() {
        let parsed = ParsedDeal {
            platform: Platform::Shopee,
            product_title: "Dầu bôi trơn Rubik".to_string(),
            product_price_vnd: Some(65000.0),
            product_shop: None,
            product_brand: None,
            product_image_url: None,
            original_url: "https://s.shopee.vn/9zzaB4uv6M".to_string(),
            resolved_url: "https://shopee.vn/product/1/2".to_string(),
            clean_url: "https://shopee.vn/product/1/2".to_string(),
            affiliate_url: "https://shopee.vn/product/1/2".to_string(),
            is_affiliate: true,
            affiliate_note: None,
            click_id: "c1".to_string(),
            sub_id: "s1".to_string(),
            primary_voucher: SaleVoucher {
                name: "Test".to_string(),
                discount_desc: "Desc".to_string(),
                min_spend_desc: "0đ".to_string(),
                remaining_percent: 80,
                back_hours: "12H".to_string(),
            },
            estimated_cashback_text: String::new(),
            tips: vec![],
        };

        let make = |id: i64, code: &str, desc: &str, url: &str, min_spend: f64| DealItem {
            id,
            platform: "Shopee".to_string(),
            title: "[Toàn sàn] Test".to_string(),
            discount_desc: desc.to_string(),
            coupon_code: Some(code.to_string()),
            min_spend,
            deal_url: url.to_string(),
            is_exclusive: true,
            is_active: true,
            expire_at: None,
        };

        let deals = vec![
            make(1, "KOL_FB_22", "📌 Hướng dẫn lấy mã độc quyền Shopee x Facebook 22%", "https://shopee.vn/m/kol", 0.0),
            make(2, "KOL_FB_20", "Voucher 20% toàn sàn", "https://shopee.vn/m/kol", 0.0),
            make(3, "SHP_AUTO_1", "Mã 23% tối đa 500k đơn từ 150K", "https://shopee.vn/search?evcode=ABC1&promotionId=111", 150000.0),
            make(4, "SHP_AUTO_2", "Mã 30% tối đa 100k đơn từ 50K", "https://shopee.vn/search?evcode=ABC2&promotionId=222", 50000.0),
            make(5, "SHP_AUTO_DUP", "Mã 25% trùng URL với SHP_AUTO_2", "https://shopee.vn/search?evcode=ABC2&promotionId=222", 50000.0),
        ];

        let top = select_top_relevant_deals(&deals, &parsed, 2);
        // Hướng dẫn #1 bị loại; #3 cần đơn 150k > giá 65k nên bị loại; #5 trùng URL #4 nên bị loại!
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].coupon_code.as_deref(), Some("SHP_AUTO_2"));
        assert_eq!(top[1].coupon_code.as_deref(), Some("KOL_FB_20"));
    }
}

use std::sync::Arc;
use chrono::Utc;
use teloxide::{
    prelude::*,
    types::{InlineKeyboardMarkup, LinkPreviewOptions, ParseMode},
    utils::command::BotCommands,
};
use tracing::{error, warn};

use super::keyboards::{
    deal_action_keyboard, deals_pagination_keyboard, main_menu_keyboard, persistent_reply_keyboard,
};
use crate::config::Config;
use crate::db::models::ClickRecord;
use crate::db::repository::Repository;
use crate::db::AppDb;
use crate::services::accesstrade::ApprovedCampaigns;
use crate::services::deal_parser::{AffiliateSettings, DealParser};
use crate::utils::{escape_html, format_vnd};

#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase", description = "Danh sách lệnh hỗ trợ:")]
pub enum Command {
    #[command(description = "Bắt đầu / Hướng dẫn dán link săn sale")]
    Start,
    #[command(description = "Kho mã giảm giá 20% - 35%+ & Mã độc quyền")]
    Deals,
    #[command(description = "Xem hướng dẫn chi tiết cách áp mã giảm giá")]
    Help,
    #[command(description = "Chia sẻ DealX cho bạn bè cùng săn sale")]
    Share,
    #[command(description = "Xem thống kê hoạt động bot (Dành cho Admin)")]
    Stats,
}

fn no_link_preview() -> LinkPreviewOptions {
    LinkPreviewOptions {
        is_disabled: true,
        url: None,
        prefer_small_media: false,
        prefer_large_media: false,
        show_above_text: false,
    }
}

/// Xử lý tin nhắn văn bản thông thường
pub async fn handle_message(
    bot: Bot,
    msg: Message,
    db: AppDb,
    config: Config,
    parser: Arc<DealParser>,
) -> ResponseResult<()> {
    let text = match msg.text() {
        Some(t) => t.trim(),
        None => return Ok(()),
    };

    let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
    let username = msg.from.as_ref().and_then(|u| u.username.as_deref());
    let full_name = msg
        .from
        .as_ref()
        .map(|u| u.full_name())
        .unwrap_or_else(|| "Bạn".to_string());

    // Xử lý các nút bấm từ Reply Keyboard thường trực ở đáy màn hình
    let mapped_cmd = match text {
        "🚀 Bắt Đầu" | "Bắt Đầu" | "start" => Some(Command::Start),
        "🎟️ Kho Mã 20% - 35%+" | "🎟️ Kho Mã 20% - 35%" | "Kho Mã 20% - 35%" | "deals" | "ma" => Some(Command::Deals),
        "📖 Hướng Dẫn Săn Sale" | "Hướng Dẫn" | "help" => Some(Command::Help),
        "🚀 Chia Sẻ Cho Bạn Bè" | "Chia Sẻ" | "share" => Some(Command::Share),
        _ => None,
    };

    if let Some(cmd) = mapped_cmd {
        return handle_command(bot, msg, cmd, db, config, parser).await;
    }

    // Hỗ trợ gõ '/deals <từ khóa>' hoặc '/ma <từ khóa>' để lọc/tìm kiếm trong toàn bộ kho mã
    let lower_text = text.to_lowercase();
    if let Some(kw) = lower_text
        .strip_prefix("/deals ")
        .or_else(|| lower_text.strip_prefix("/ma "))
    {
        let kw = kw.trim();
        if !kw.is_empty() {
            if let Ok(conn) = db.conn() {
                let (html, keyboard) = build_deals_page(&conn, &config, &parser, kw, 0, false).await;
                bot.send_message(msg.chat.id, html)
                    .parse_mode(ParseMode::Html)
                    .link_preview_options(no_link_preview())
                    .reply_markup(keyboard)
                    .await?;
                return Ok(());
            }
        }
    }

    // Nếu tin nhắn bắt đầu bằng lệnh '/'
    if text.starts_with('/') {
        if let Ok(cmd) = Command::parse(text, &config.bot_username) {
            return handle_command(bot, msg, cmd, db, config, parser).await;
        }
    }

    // Kiểm tra xem tin nhắn có chứa URL link sản phẩm không
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

        // Gửi thông báo đang tìm và gắn mã giảm giá
        let wait_msg = bot
            .send_message(msg.chat.id, "🔍 <i>Đang quét và gắn mã giảm giá tốt nhất cho sản phẩm này...</i>")
            .parse_mode(ParseMode::Html)
            .await?;

        // Lấy cấu hình ACCESSTRADE API Token & Campaign ID từ Web Admin hoặc biến môi trường
        let affiliate_template = Repository::get_setting(&conn, "affiliate_template")
            .await
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| config.accesstrade_deeplink_template.clone());

        let db_at_token = Repository::get_setting(&conn, "accesstrade_api_token")
            .await
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| config.accesstrade_api_token.clone());

        let campaign_shopee = Repository::get_setting(&conn, "campaign_shopee").await.ok().flatten().filter(|s| !s.trim().is_empty());
        let campaign_lazada = Repository::get_setting(&conn, "campaign_lazada").await.ok().flatten().filter(|s| !s.trim().is_empty());
        let campaign_tiki = Repository::get_setting(&conn, "campaign_tiki").await.ok().flatten().filter(|s| !s.trim().is_empty());
        let campaign_tiktok = Repository::get_setting(&conn, "campaign_tiktok").await.ok().flatten().filter(|s| !s.trim().is_empty());

        let aff_settings = AffiliateSettings {
            deeplink_template: &affiliate_template,
            accesstrade_token: db_at_token.as_deref(),
            campaign_shopee: campaign_shopee.as_deref(),
            campaign_lazada: campaign_lazada.as_deref(),
            campaign_tiki: campaign_tiki.as_deref(),
            campaign_tiktok: campaign_tiktok.as_deref(),
        };

        // Bóc tách link và gọi ACCESSTRADE API tạo link thực tế
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

                let mut tips_text = String::new();
                for tip in &parsed.tips {
                    tips_text.push_str(&format!("- {}\n", escape_html(tip)));
                }

                let safe_aff_url = escape_html(&parsed.affiliate_url);
                let header_line = if parsed.is_affiliate {
                    format!(
                        "👉 <b>{} ({}) · 🟢 {}% :</b>\n{} (<code>{}</code>)",
                        escape_html(&parsed.primary_voucher.name),
                        escape_html(&parsed.primary_voucher.discount_desc),
                        parsed.primary_voucher.remaining_percent,
                        safe_aff_url,
                        escape_html(&parsed.primary_voucher.min_spend_desc)
                    )
                } else {
                    format!(
                        "👉 <b>{}</b>\n🔗 <i>Link mở sản phẩm:</i> {}\n<i>({})</i>",
                        escape_html(&parsed.primary_voucher.name),
                        safe_aff_url,
                        escape_html(&parsed.primary_voucher.discount_desc)
                    )
                };

                let price_line = match parsed.product_price_vnd {
                    Some(price) if price > 0.0 => format!("💰 <i>Giá tham khảo:</i> <b>{}</b>\n", escape_html(&format_vnd(price))),
                    _ => String::new(),
                };

                // Kiểm tra xem sàn này có mã độc quyền hợp tác hoặc mã thị trường khớp sản phẩm/sàn không
                let mut extra_vouchers_block = match Repository::get_active_deals(&conn, Some(parsed.platform.as_str())).await {
                    Ok(deals) => {
                        let ex_list: Vec<_> = deals.into_iter().take(2).collect();
                        if ex_list.is_empty() {
                            String::new()
                        } else {
                            let mut s = String::from("\n\n👑 <b>MÃ ĐỘC QUYỀN HỢP TÁC ĐANG CÓ:</b>");
                            for d in ex_list {
                                let code_tag = d
                                    .coupon_code
                                    .as_deref()
                                    .filter(|c| !c.trim().is_empty())
                                    .map(|c| format!(" · Mã: <code>{}</code>", escape_html(c)))
                                    .unwrap_or_default();
                                s.push_str(&format!(
                                    "\n• <b>{}</b> ({}){}",
                                    escape_html(&d.title),
                                    escape_html(&d.discount_desc),
                                    code_tag
                                ));
                            }
                            s
                        }
                    }
                    Err(_) => String::new(),
                };

                let matched_market = parser
                    .accesstrade()
                    .search_matching_vouchers_for_deal(
                        parsed.platform.as_str(),
                        &parsed.product_title,
                        &parsed.clean_url,
                        3,
                    )
                    .await;
                if !matched_market.is_empty() {
                    extra_vouchers_block.push_str("\n\n🎟️ <b>MÃ GIẢM GIÁ THỊ TRƯỜNG ĐANG CÓ TRÊN SÀN:</b>");
                    for mv in matched_market {
                        extra_vouchers_block.push_str(&format!(
                            "\n• <b>[{}] {}</b> · Mã: <code>{}</code> ({})",
                            escape_html(&mv.discount_badge()),
                            escape_html(&mv.shop_name),
                            escape_html(&mv.coupon_code),
                            escape_html(&mv.discount_desc)
                        ));
                    }
                }

                let reply_html = format!(
                    "📦 <b>{}</b>\n\
                     {}\
                     🔗 <i>Link gốc:</i> {}\n\n\
                     {}{}\n\n\
                     ⏰ <b>BACK MÃ LÚC:</b> {}\n\n\
                     ⚠️ <b>Lưu ý để áp mã thành công 100%:</b>\n\
                     {}\n\
                     👉 <i>Bấm các nút bên dưới để lưu mã &amp; mở App sàn mua hàng ngay:</i>",
                    escape_html(&parsed.product_title),
                    price_line,
                    escape_html(&parsed.original_url),
                    header_line,
                    extra_vouchers_block,
                    escape_html(&parsed.primary_voucher.back_hours),
                    tips_text.trim_end()
                );

                // Xóa tin nhắn chờ và gửi kết quả
                let _ = bot.delete_message(msg.chat.id, wait_msg.id).await;

                bot.send_message(msg.chat.id, reply_html)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(deal_action_keyboard(&parsed.affiliate_url, parsed.platform.as_str()))
                    .await?;
            }
            Err(err) => {
                let _ = bot.delete_message(msg.chat.id, wait_msg.id).await;
                let err_text = format!(
                    "⚠️ <b>Không thể phân tích link này</b>\n\n\
                     Chi tiết: {}\n\n\
                     👉 Vui lòng dán đường link sản phẩm từ <b>Shopee</b>, <b>Lazada</b>, <b>Tiki</b> hoặc <b>TikTok Shop</b>.",
                    escape_html(&err.to_string())
                );
                bot.send_message(msg.chat.id, err_text)
                    .parse_mode(ParseMode::Html)
                    .await?;
            }
        }
        return Ok(());
    }

    // Tin nhắn thông thường không có link
    let reply = format!(
        "👋 Chào <b>{}</b>!\n\n\
         🤖 <b>Bot Tự Động Săn Mã Giảm Giá Đa Sàn (Shopee, Lazada, Tiki, TikTok Shop)</b>\n\n\
         📌 <b>Cách lấy mã giảm giá cực đơn giản:</b>\n\
         1️⃣ Mở app <b>Shopee, Lazada, Tiki hoặc TikTok Shop</b> và copy link món đồ bạn muốn mua.\n\
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

/// Xử lý các lệnh Slash Commands (/start, /deals, /help, /share, /stats)
pub async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    db: AppDb,
    config: Config,
    parser: Arc<DealParser>,
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

            let welcome = "⚡ <b>CHÀO MỪNG BẠN ĐẾN VỚI DEALX - TRỢ LÝ SĂN MÃ &amp; ÉP GIÁ</b> ⚡\n\n\
                 🔥 <b>Mua hàng tiết kiệm hơn chỉ với 1 bước:</b>\n\
                 Chỉ cần <b>Dán bất kỳ link Shopee, Lazada, Tiki hoặc TikTok Shop</b> nào vào khung chat này, hoặc bấm nút <b>🎟️ Kho Mã 20% - 35%+</b> bên dưới để lấy hàng loạt mã giảm giá thật mới nhất!\n\n\
                 👇 <i>Hãy gửi thử 1 link sản phẩm hoặc bấm Kho Mã ngay:</i>";

            bot.send_message(msg.chat.id, welcome)
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username))
                .await?;
        }

        Command::Deals => {
            let wait_msg = bot
                .send_message(msg.chat.id, "🎟️ <i>Đang tải kho mã giảm giá 20% - 35%+ &amp; mã độc quyền...</i>")
                .parse_mode(ParseMode::Html)
                .await?;

            let (html, keyboard) = build_deals_page(&conn, &config, &parser, "all", 0, false).await;
            let _ = bot.delete_message(msg.chat.id, wait_msg.id).await;

            bot.send_message(msg.chat.id, html)
                .parse_mode(ParseMode::Html)
                .link_preview_options(no_link_preview())
                .reply_markup(keyboard)
                .await?;
        }

        Command::Help => {
            let help_text = 
                "📖 <b>HƯỚNG DẪN 3 BƯỚC NHẬN MÃ GIẢM GIÁ ĐƠN GIẢN:</b>\n\n\
                 1️⃣ <b>Bước 1:</b> Vào App Shopee, Lazada, Tiki hoặc TikTok Shop chọn sản phẩm bạn muốn mua.\n\
                 2️⃣ <b>Bước 2:</b> Bấm nút <b>Chia sẻ</b> -&gt; chọn <b>Sao chép liên kết</b>.\n\
                 3️⃣ <b>Bước 3:</b> Dán đường link vào khung chat bot này. Bot sẽ trả về link mua hàng kèm cổng lưu mã.\n\n\
                 🎟️ Ngoài ra, gõ <code>/deals</code> để lấy kho mã <b>giảm 20% - 35%+</b> và <b>mã độc quyền hợp tác</b> cập nhật liên tục!\n\n\
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
                "⚡ <b>CHIA SẺ DEALX CHO BẠN BÈ CÙNG SĂN SALE</b> ⚡\n\n\
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
            let stats = Repository::get_admin_dashboard_stats(&conn).await.unwrap_or_default();

            let stats_html = format!(
                "📊 <b>BÁO CÁO HOẠT ĐỘNG BOT (DÀNH CHO CHỦ BOT)</b>\n\n\
                 👥 <b>Tổng số người dùng bot:</b> <b>{} người</b>\n\
                 🔗 <b>Tổng số lượt dán link &amp; tạo mã:</b> <b>{} lượt</b>\n\n\
                 💰 <b>Hoa hồng tiếp thị:</b>\n\
                 100% doanh thu chiết khấu từ các lượt click trên sẽ được sàn TMĐT đối soát và chuyển trực tiếp vào tài khoản AccessTrade của bạn!",
                stats.total_users, stats.total_clicks
            );

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
    db: AppDb,
    config: Config,
    parser: Arc<DealParser>,
) -> ResponseResult<()> {
    let data = match q.data.as_deref() {
        Some(d) => d,
        None => return Ok(()),
    };

    let (chat_id, message_id) = match q.message {
        Some(ref msg) => (msg.chat().id, msg.id()),
        None => return Ok(()),
    };

    let conn = match db.conn() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let _ = bot.answer_callback_query(q.id).await;

    if let Some(rest) = data.strip_prefix("deals_page:") {
        let mut parts = rest.splitn(2, ':');
        let filter = parts.next().unwrap_or("all");
        let page = parts.next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
        let (html, keyboard) = build_deals_page(&conn, &config, &parser, filter, page, false).await;
        if let Err(err) = bot
            .edit_message_text(chat_id, message_id, &html)
            .parse_mode(ParseMode::Html)
            .link_preview_options(no_link_preview())
            .reply_markup(keyboard.clone())
            .await
        {
            // Nếu Telegram từ chối edit do quá hạn hoặc nội dung không đổi -> bỏ qua hoặc gửi mới
            let err_str = err.to_string();
            if !err_str.contains("message is not modified") {
                let _ = bot
                    .send_message(chat_id, html)
                    .parse_mode(ParseMode::Html)
                    .link_preview_options(no_link_preview())
                    .reply_markup(keyboard)
                    .await;
            }
        }
        return Ok(());
    }

    if let Some(rest) = data.strip_prefix("deals_refresh:") {
        let mut parts = rest.splitn(2, ':');
        let filter = parts.next().unwrap_or("all");
        let page = parts.next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
        let (html, keyboard) = build_deals_page(&conn, &config, &parser, filter, page, true).await;
        let _ = bot
            .edit_message_text(chat_id, message_id, html)
            .parse_mode(ParseMode::Html)
            .link_preview_options(no_link_preview())
            .reply_markup(keyboard)
            .await;
        return Ok(());
    }

    match data {
        "menu_guide" => {
            let guide = "📖 <b>HƯỚNG DẪN 3 BƯỚC NHẬN MÃ GIẢM GIÁ:</b>\n\n\
                         1️⃣ Copy link sản phẩm trên app Shopee, Lazada, Tiki hoặc TikTok Shop.\n\
                         2️⃣ Dán link vào khung chat này để bot tạo link ưu đãi.\n\
                         3️⃣ Bấm nút mua hàng để mở app sàn và đặt hàng với giá đã giảm!\n\n\
                         <i>Mẹo: Xóa sản phẩm cũ trong giỏ hàng trước khi mua để tránh bị lỗi mã!</i>";
            bot.send_message(chat_id, guide)
                .parse_mode(ParseMode::Html)
                .await?;
        }
        "menu_main" => {
            bot.send_message(chat_id, "🏠 <b>MENU CHÍNH DEALX</b>")
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username))
                .await?;
        }
        _ => {}
    }

    Ok(())
}

enum DealDisplayItem<'a> {
    Exclusive(&'a crate::db::models::DealItem),
    ApiOffer(&'a crate::services::accesstrade::VoucherOffer),
}

/// Dựng giao diện trang danh sách Toàn Bộ Mã Giảm Giá Trên Thị Trường (Độc quyền + 20%-35%+ + Giảm tiền/Khác)
async fn build_deals_page(
    conn: &libsql::Connection,
    config: &Config,
    parser: &DealParser,
    filter: &str,
    requested_page: usize,
    force_refresh: bool,
) -> (String, InlineKeyboardMarkup) {
    const PAGE_SIZE: usize = 6;

    let db_deals = Repository::get_active_deals(conn, None).await.unwrap_or_default();

    let token_opt = Repository::get_setting(conn, "accesstrade_api_token")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| config.accesstrade_api_token.clone());

    let at_client = parser.accesstrade();
    // Luôn lấy với min_discount_pct = 0 để thu thập 100% mã trên thị trường không bỏ sót bất kỳ mã nào
    let (approved, all_api_vouchers, api_err): (Option<ApprovedCampaigns>, Vec<_>, Option<String>) =
        if let Some(ref tok) = token_opt {
            let approved = at_client.get_approved_campaigns(tok, false).await.ok();
            match at_client.fetch_high_discount_vouchers(tok, 0, force_refresh).await {
                Ok(v) => (approved, v, None),
                Err(err) => (approved, Vec::new(), Some(err)),
            }
        } else {
            (None, Vec::new(), Some("Chưa cấu hình ACCESSTRADE Token".to_string()))
        };

    let high_pct_count = all_api_vouchers.iter().filter(|v| v.discount_percent >= 20).count();
    let other_count = all_api_vouchers.len().saturating_sub(high_pct_count);

    let mut entries: Vec<DealDisplayItem<'_>> = Vec::new();
    let filter_norm = filter.trim().to_lowercase();
    match filter_norm.as_str() {
        "exclusive" => {
            for d in &db_deals {
                entries.push(DealDisplayItem::Exclusive(d));
            }
        }
        "pct20" => {
            for v in all_api_vouchers.iter().filter(|v| v.discount_percent >= 20) {
                entries.push(DealDisplayItem::ApiOffer(v));
            }
        }
        "other" => {
            for v in all_api_vouchers.iter().filter(|v| v.discount_percent < 20) {
                entries.push(DealDisplayItem::ApiOffer(v));
            }
        }
        "shopee" => {
            for d in db_deals.iter().filter(|d| d.platform.eq_ignore_ascii_case("Shopee")) {
                entries.push(DealDisplayItem::Exclusive(d));
            }
            for v in all_api_vouchers.iter().filter(|v| v.platform.eq_ignore_ascii_case("Shopee")) {
                entries.push(DealDisplayItem::ApiOffer(v));
            }
        }
        "tiki" => {
            for d in db_deals.iter().filter(|d| d.platform.eq_ignore_ascii_case("Tiki")) {
                entries.push(DealDisplayItem::Exclusive(d));
            }
            for v in all_api_vouchers.iter().filter(|v| v.platform.eq_ignore_ascii_case("Tiki")) {
                entries.push(DealDisplayItem::ApiOffer(v));
            }
        }
        "all" | "" => {
            for d in &db_deals {
                entries.push(DealDisplayItem::Exclusive(d));
            }
            for v in &all_api_vouchers {
                entries.push(DealDisplayItem::ApiOffer(v));
            }
        }
        kw => {
            for d in db_deals.iter().filter(|d| {
                d.title.to_lowercase().contains(kw)
                    || d.platform.to_lowercase().contains(kw)
                    || d.discount_desc.to_lowercase().contains(kw)
                    || d.coupon_code.as_deref().unwrap_or("").to_lowercase().contains(kw)
            }) {
                entries.push(DealDisplayItem::Exclusive(d));
            }
            for v in all_api_vouchers.iter().filter(|v| {
                v.shop_name.to_lowercase().contains(kw)
                    || v.platform.to_lowercase().contains(kw)
                    || v.discount_desc.to_lowercase().contains(kw)
                    || v.coupon_code.to_lowercase().contains(kw)
            }) {
                entries.push(DealDisplayItem::ApiOffer(v));
            }
        }
    }

    let total_items = entries.len();
    let total_pages = total_items.div_ceil(PAGE_SIZE).max(1);
    let page = requested_page.min(total_pages.saturating_sub(1));
    let start = page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(total_items);

    let vn_now = Utc::now() + chrono::Duration::hours(7);
    let filter_label = match filter_norm.as_str() {
        "exclusive" => "👑 Mã Độc Quyền Hợp Tác".to_string(),
        "pct20" => "🔥 Kho Mã Giảm Sâu 20% - 35%+".to_string(),
        "other" => "💸 Mã Giảm Tiền Mặt & 1% - 19%".to_string(),
        "shopee" => "🟠 Toàn Bộ Mã Shopee".to_string(),
        "tiki" => "🔹 Toàn Bộ Mã Tiki".to_string(),
        "all" | "" => "🌟 Toàn Bộ Mã Trên Thị Trường (Không Bỏ Sót)".to_string(),
        kw => format!("🔍 Tìm kiếm: \"{}\"", escape_html(kw)),
    };

    let mut html = format!(
        "🎟️ <b>KHO MÃ GIẢM GIÁ TOÀN THỊ TRƯỜNG — {}</b>\n\
         👑 Độc quyền: <b>{}</b> | 🔥 &gt;=20%: <b>{}</b> | 💸 Mã khác: <b>{}</b> | Tổng: <b>{}</b> (<i>{}</i>)\n\
         <i>(Chạm vào <code>MÃ CODE</code> để sao chép · Gõ <code>/deals từ_khóa</code> để tìm shop)</i>\n",
        filter_label,
        db_deals.len(),
        high_pct_count,
        other_count,
        db_deals.len() + all_api_vouchers.len(),
        vn_now.format("%H:%M %d/%m")
    );

    if let Some(err) = api_err {
        html.push_str(&format!("\n⚠️ <i>Lưu ý đồng bộ API: {}</i>\n", escape_html(&err)));
    }

    if total_items == 0 {
        if filter_norm == "exclusive" {
            html.push_str(
                "\n📭 <i>Chưa có Mã Độc Quyền Hợp Tác nào được nhập. Chủ bot có thể dán hàng loạt mã từ AM/MCN hoặc Shop hợp tác vào mục <b>Kho Mã Độc Quyền</b> trên Web Admin.</i>",
            );
        } else {
            html.push_str("\n📭 <i>Không tìm thấy mã nào khớp bộ lọc. Hãy thử bấm 🌟 Tất Cả hoặc 🔄 Cập Nhật.</i>");
        }
    } else {
        for (idx, item) in entries[start..end].iter().enumerate() {
            let num = start + idx + 1;
            match item {
                DealDisplayItem::Exclusive(d) => {
                    let code_str = d
                        .coupon_code
                        .as_deref()
                        .filter(|c| !c.trim().is_empty())
                        .map(|c| format!(" · Mã: <code>{}</code>", escape_html(c)))
                        .unwrap_or_default();
                    let min_str = if d.min_spend > 0.0 {
                        format!(" · Đơn từ {}", escape_html(&format_vnd(d.min_spend)))
                    } else {
                        String::new()
                    };
                    let tag = if d.is_exclusive { "ĐỘC QUYỀN" } else { "DEAL HOT" };
                    html.push_str(&format!(
                        "\n{}. 👑 <b>[{} · {}] {}</b>{}\n   💬 {}{}\n   👉 <a href=\"{}\">Mở &amp; Áp Mã Ngay ↗</a>\n",
                        num,
                        tag,
                        escape_html(&d.platform),
                        escape_html(&d.title),
                        code_str,
                        escape_html(&d.discount_desc),
                        min_str,
                        escape_html(&d.deal_url)
                    ));
                }
                DealDisplayItem::ApiOffer(v) => {
                    let target_url = v.best_target_url(approved.as_ref());
                    let badge = v.discount_badge();
                    html.push_str(&format!(
                        "\n{}. 🔥 <b>[{} · {}] {}</b>\n   🎟️ Mã: <code>{}</code> · HSD: {}\n   💬 {}\n   👉 <a href=\"{}\">Mở Gian Hàng &amp; Áp Mã ↗</a>\n",
                        num,
                        escape_html(&badge),
                        escape_html(&v.platform),
                        escape_html(&v.shop_name),
                        escape_html(&v.coupon_code),
                        escape_html(&v.end_time),
                        escape_html(&v.discount_desc),
                        escape_html(target_url)
                    ));
                }
            }
        }
    }

    let kb_filter = match filter_norm.as_str() {
        "exclusive" | "pct20" | "other" | "shopee" | "tiki" | "all" => filter_norm.as_str(),
        _ => "all",
    };
    let keyboard = deals_pagination_keyboard(kb_filter, page, total_pages);
    (html, keyboard)
}

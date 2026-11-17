use std::sync::Arc;
use chrono::Utc;
use teloxide::{
    prelude::*,
    types::ParseMode,
    utils::command::BotCommands,
};
use tracing::{error, warn};

use super::keyboards::{deal_action_keyboard, main_menu_keyboard, persistent_reply_keyboard};
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
    #[command(description = "Chia sẻ DealX cho bạn bè cùng săn sale")]
    Share,
    #[command(description = "Xem thống kê hoạt động bot (Dành cho Admin)")]
    Stats,
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
        "📖 Hướng Dẫn Săn Sale" | "Hướng Dẫn" | "help" => Some(Command::Help),
        "🚀 Chia Sẻ Cho Bạn Bè" | "Chia Sẻ" | "share" => Some(Command::Share),
        _ => None,
    };

    if let Some(cmd) = mapped_cmd {
        return handle_command(bot, msg, cmd, db, config, parser).await;
    }

    // Nếu người dùng bấm nút Kho Mã cũ hoặc gõ /deals /ma -> hướng dẫn dán link sản phẩm trực tiếp
    let lower_text = text.to_lowercase();
    if text == "🎟️ Kho Mã 20% - 35%+"
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
                    let note_line = parsed
                        .affiliate_note
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| format!("\n⚠️ <i>Trạng thái link Affiliate: {}</i>", escape_html(s)))
                        .unwrap_or_default();
                    format!(
                        "👉 <b>{}</b>\n🔗 <i>Link mở sản phẩm:</i> {}\n<i>({})</i>{}",
                        escape_html(&parsed.primary_voucher.name),
                        safe_aff_url,
                        escape_html(&parsed.primary_voucher.discount_desc),
                        note_line
                    )
                };

                let shop_line = match (
                    parsed.product_shop.as_deref(),
                    parsed.product_brand.as_deref(),
                ) {
                    (Some(shop), Some(brand)) => format!(
                        "🏪 <i>Shop:</i> <b>{}</b> · <i>Thương hiệu:</i> <b>{}</b>\n",
                        escape_html(shop),
                        escape_html(brand)
                    ),
                    (Some(shop), None) => format!("🏪 <i>Shop:</i> <b>{}</b>\n", escape_html(shop)),
                    (None, Some(brand)) => format!("🏷️ <i>Thương hiệu:</i> <b>{}</b>\n", escape_html(brand)),
                    (None, None) => String::new(),
                };

                let price_line = match parsed.product_price_vnd {
                    Some(price) if price > 0.0 => format!("💰 <i>Giá tham khảo:</i> <b>{}</b>\n", escape_html(&format_vnd(price))),
                    _ => String::new(),
                };

                let mut voucher_buttons: Vec<(String, String)> = Vec::new();

                // Chỉ hiển thị & đính link trực tiếp cho mã khớp ĐÚNG với shop / thương hiệu / toàn sàn của sản phẩm này
                let mut extra_vouchers_block = match Repository::get_active_deals(&conn, Some(parsed.platform.as_str())).await {
                    Ok(deals) => {
                        let ex_list: Vec<_> = deals
                            .into_iter()
                            .filter(|d| is_exclusive_deal_relevant(d, &parsed))
                            .take(2)
                            .collect();
                        if ex_list.is_empty() {
                            String::new()
                        } else {
                            let mut s = String::from("\n\n👑 <b>MÃ ĐỘC QUYỀN ÁP DỤNG CHO SẢN PHẨM NÀY:</b>");
                            for d in ex_list {
                                let code_tag = d
                                    .coupon_code
                                    .as_deref()
                                    .filter(|c| !c.trim().is_empty())
                                    .map(|c| format!(" · Mã: <code>{}</code>", escape_html(c)))
                                    .unwrap_or_default();
                                s.push_str(&format!(
                                    "\n• <b>{}</b> ({}){}\n  👉 <a href=\"{}\">Mở &amp; Áp Mã Ngay ↗</a>",
                                    escape_html(&d.title),
                                    escape_html(&d.discount_desc),
                                    code_tag,
                                    escape_html(&d.deal_url)
                                ));
                                if voucher_buttons.len() < 2 && !d.deal_url.trim().is_empty() {
                                    let btn_label = match d.coupon_code.as_deref().filter(|c| !c.trim().is_empty()) {
                                        Some(c) => format!("👑 Lấy Mã Độc Quyền · {}", c),
                                        None => format!("👑 {}", d.title),
                                    };
                                    voucher_buttons.push((btn_label, d.deal_url));
                                }
                            }
                            s
                        }
                    }
                    Err(_) => String::new(),
                };

                let approved_cached = parser.accesstrade().peek_approved_campaigns().await;
                let matched_market = parser
                    .accesstrade()
                    .search_matching_vouchers_for_deal(
                        parsed.platform.as_str(),
                        &parsed.product_title,
                        &parsed.clean_url,
                        parsed.product_shop.as_deref(),
                        parsed.product_brand.as_deref(),
                        parsed.product_price_vnd,
                        2,
                    )
                    .await;
                if !matched_market.is_empty() {
                    extra_vouchers_block.push_str("\n\n🎟️ <b>MÃ KHỚP ĐÚNG SHOP / TOÀN SÀN CHO SẢN PHẨM NÀY:</b>");
                    for mv in matched_market {
                        let direct_voucher_url = parser
                            .accesstrade()
                            .verified_offer_url(&mv, approved_cached.as_ref())
                            .await;
                        extra_vouchers_block.push_str(&format!(
                            "\n• <b>[{}] {}</b> · Mã: <code>{}</code>\n  💬 {} · 👉 <a href=\"{}\">Mở Shop &amp; Lấy Mã ↗</a>",
                            escape_html(&mv.discount_badge()),
                            escape_html(&mv.shop_name),
                            escape_html(&mv.coupon_code),
                            escape_html(&mv.discount_desc),
                            escape_html(&direct_voucher_url)
                        ));
                        if voucher_buttons.len() < 2 {
                            voucher_buttons.push((
                                format!("🎟️ Lấy Mã {} · {}", mv.discount_badge(), mv.coupon_code),
                                direct_voucher_url,
                            ));
                        }
                    }
                }

                let reply_html = format!(
                    "📦 <b>{}</b>\n\
                     {}{}\
                     🔗 <i>Link gốc:</i> {}\n\n\
                     {}{}\n\n\
                     ⏰ <b>BACK MÃ LÚC:</b> {}\n\n\
                     ⚠️ <b>Lưu ý để áp mã thành công 100%:</b>\n\
                     {}\n\
                     👉 <i>Bấm các nút bên dưới để mở sản phẩm &amp; lấy mã khớp trực tiếp:</i>",
                    escape_html(&parsed.product_title),
                    shop_line,
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
                    .reply_markup(deal_action_keyboard(
                        &parsed.affiliate_url,
                        parsed.platform.as_str(),
                        &voucher_buttons,
                    ))
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

            let welcome = "⚡ <b>CHÀO MỪNG BẠN ĐẾN VỚI DEALX - TRỢ LÝ SĂN MÃ &amp; GẮN LINK</b> ⚡\n\n\
                 🔥 <b>Mua hàng tiết kiệm hơn chỉ với 1 bước:</b>\n\
                 Chỉ cần <b>Dán trực tiếp link sản phẩm Shopee, Lazada, Tiki hoặc TikTok Shop</b> vào khung chat này, bot sẽ tự phân tích sản phẩm/shop và gắn link mua kèm mã khớp riêng (nếu có) ngay cho bạn!\n\n\
                 👇 <i>Hãy dán thử 1 link sản phẩm vào đây ngay:</i>";

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
            bot.send_message(chat_id, "🏠 <b>MENU CHÍNH DEALX</b>")
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username))
                .await?;
        }
        _ => {}
    }

    Ok(())
}

/// Kiểm tra mã độc quyền trong DB có thực sự áp được cho sản phẩm đang dán (toàn sàn hoặc khớp shop/từ khóa) hay không
pub fn is_exclusive_deal_relevant(deal: &crate::db::models::DealItem, parsed: &ParsedDeal) -> bool {
    if !deal.platform.eq_ignore_ascii_case(parsed.platform.as_str()) {
        return false;
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
}

use std::sync::Arc;
use chrono::Utc;
use teloxide::{
    prelude::*,
    types::ParseMode,
    utils::command::BotCommands,
};
use tracing::{error, warn};

use super::keyboards::{deal_action_keyboard, deals_filter_keyboard, main_menu_keyboard, persistent_reply_keyboard};
use crate::config::Config;
use crate::db::models::ClickRecord;
use crate::db::repository::Repository;
use crate::db::AppDb;
use crate::services::deal_parser::DealParser;
use crate::utils::format_vnd;

#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase", description = "Danh sách lệnh hỗ trợ:")]
pub enum Command {
    #[command(description = "Bắt đầu sử dụng bot săn mã giảm giá")]
    Start,
    #[command(description = "Xem hướng dẫn chi tiết cách áp mã giảm giá")]
    Help,
    #[command(description = "Xem mã giảm giá & deals hot (Shopee, Lazada, Tiki)")]
    Deals(String),
    #[command(description = "Chia sẻ bot săn sale cho bạn bè")]
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
        "🎟️ Mã Giảm Giá Hôm Nay" | "Mã Giảm Giá" | "deals" => Some(Command::Deals(String::new())),
        "🟠 Săn Sale Shopee" => Some(Command::Deals("Shopee".to_string())),
        "🔵 Săn Sale Lazada" => Some(Command::Deals("Lazada".to_string())),
        "📖 Hướng Dẫn Săn Sale" | "Hướng Dẫn" | "help" => Some(Command::Help),
        "🚀 Chia Sẻ Cho Bạn Bè" | "Chia Sẻ" | "share" => Some(Command::Share),
        _ => None,
    };

    if let Some(cmd) = mapped_cmd {
        return handle_command(bot, msg, cmd, db, config).await;
    }

    // Nếu tin nhắn bắt đầu bằng lệnh '/'
    if text.starts_with('/') {
        if let Ok(cmd) = Command::parse(text, &config.bot_username) {
            return handle_command(bot, msg, cmd, db, config).await;
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

        // Bóc tách link và tạo link có mã sale (Affiliate link nhận 100% chiết khấu cho chủ bot)
        match parser
            .process_link(
                text,
                user.telegram_id,
                None, // Toàn bộ hoa hồng chiết khấu chảy trực tiếp về cho chủ bot
                &config.accesstrade_deeplink_template,
            )
            .await
        {
            Ok(parsed) => {
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
                    tips_text.push_str(&format!("- {}\n", tip));
                }

                // Định dạng chuẩn xác theo mẫu người dùng cung cấp
                let reply_html = format!(
                    "📦 <b>{}</b>\n\
                     🔗 <i>Link gốc:</i> {}\n\n\
                     👉 <b>{} ({}) · 🟢 {}% :</b>\n\
                     {} (<code>{}</code>)\n\n\
                     ⏰ <b>BACK MÃ LÚC:</b> {}\n\n\
                     ⚠️ <b>Lưu ý để áp mã thành công 100%:</b>\n\
                     {}\n\
                     👉 <i>Bấm nút bên dưới để mở App sàn và áp mã giảm giá ngay:</i>",
                    parsed.product_title,
                    parsed.original_url,
                    parsed.primary_voucher.name,
                    parsed.primary_voucher.discount_desc,
                    parsed.primary_voucher.remaining_percent,
                    parsed.affiliate_url,
                    parsed.primary_voucher.min_spend_desc,
                    parsed.primary_voucher.back_hours,
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
                    err
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
         🤖 <b>Bot Tự Động Săn Mã Giảm Giá Đa Sàn (Shopee, Lazada, Tiki)</b>\n\n\
         📌 <b>Cách lấy mã giảm giá cực đơn giản:</b>\n\
         1️⃣ Mở app <b>Shopee, Lazada, Tiki</b> và copy link món đồ bạn muốn mua.\n\
         2️⃣ Dán trực tiếp đường link vào khung chat này.\n\
         3️⃣ Bot sẽ tự động nhặt và trả lại link đã được <b>Gắn Mã Giảm 20% - 50k - 100k</b>!\n\n\
         👇 <i>Bấm các nút chức năng bên dưới hoặc gửi link sản phẩm ngay:</i>",
        full_name
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

            let welcome = format!(
                "⚡ <b>CHÀO MỪNG BẠN ĐẾN VỚI DEALX - TRỢ LÝ SĂN MÃ & ÉP GIÁ</b> ⚡\n\n\
                 🔥 <b>Mua hàng rẻ hơn 20% - 50% chỉ với 1 bước:</b>\n\
                 Chỉ cần <b>Dán bất kỳ link Shopee, Lazada, Tiki</b> nào vào khung chat này, DealX sẽ tự động gom mã <b>Facebook 20%</b>, <b>Mã Video 30% - 50%</b>, <b>Freeship</b> và gửi lại link mua hàng đã gắn mã cho bạn!\n\n\
                 👇 <i>Hãy gửi thử 1 link sản phẩm ngay bây giờ để DealX ép giá cho bạn:</i>"
            );

            bot.send_message(msg.chat.id, welcome)
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username))
                .await?;
        }

        Command::Help => {
            let help_text = 
                "📖 <b>HƯỚNG DẪN 3 BƯỚC NHẬN MÃ GIẢM GIÁ ĐƠN GIẢN:</b>\n\n\
                 1️⃣ <b>Bước 1:</b> Vào App Shopee, Lazada hoặc Tiki chọn sản phẩm bạn muốn mua.\n\
                 2️⃣ <b>Bước 2:</b> Bấm nút <b>Chia sẻ</b> -> chọn <b>Sao chép liên kết</b>.\n\
                 3️⃣ <b>Bước 3:</b> Dán đường link vào khung chat bot này. Bot sẽ trả về link đã được <b>gắn mã giảm giá</b>.\n\n\
                 ⚠️ <b>MẸO ÁP MÃ THÀNH CÔNG 100%:</b>\n\
                 • Bấm vào link bot gửi để mở App sàn.\n\
                 • Xoá sản phẩm cũ trong giỏ hàng trước khi mua để mã không bị xung đột.\n\
                 • Canh đúng các khung giờ back mã: <code>0H · 9H · 12H · 15H · 20H</code> để áp mã giảm sâu nhất!";

            bot.send_message(msg.chat.id, help_text)
                .parse_mode(ParseMode::Html)
                .await?;
        }

        Command::Deals(arg) => {
            let filter = if arg.trim().is_empty() {
                None
            } else {
                Some(arg.trim())
            };

            let deals = Repository::get_active_deals(&conn, filter)
                .await
                .unwrap_or_default();

            if deals.is_empty() {
                bot.send_message(
                    msg.chat.id,
                    "Hiện tại chưa có mã giảm giá mới cho sàn này. Vui lòng quay lại sau!",
                )
                .await?;
            } else {
                let mut text = "🔥 <b>DANH SÁCH MÃ GIẢM GIÁ & DEALS HOT HÔM NAY</b> 🔥\n\n".to_string();
                for (i, d) in deals.iter().take(6).enumerate() {
                    let code_str = d
                        .coupon_code
                        .as_deref()
                        .map(|c| format!("Mã: <code>{}</code>", c))
                        .unwrap_or_else(|| "Tự động áp dụng".to_string());

                    text.push_str(&format!(
                        "<b>{}. [{}] {}</b>\n\
                         🏷️ {}\n\
                         🎁 <i>{}</i> (Đơn từ {})\n\
                         👉 <a href=\"{}\">Lưu mã ngay</a>\n\n",
                        i + 1,
                        d.platform,
                        d.title,
                        code_str,
                        d.discount_desc,
                        format_vnd(d.min_spend),
                        d.deal_url
                    ));
                }

                bot.send_message(msg.chat.id, text)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(deals_filter_keyboard())
                    .await?;
            }
        }

        Command::Share => {
            let share_link = format!("https://t.me/{}", config.bot_username);

            let share_html = format!(
                "⚡ <b>CHIA SẺ DEALX CHO BẠN BÈ CÙNG SĂN SALE</b> ⚡\n\n\
                 Giúp bạn bè mua sắm tiết kiệm từ 20% - 50% trên Shopee, Lazada, Tiki mỗi ngày!\n\n\
                 🔗 <b>Link bot:</b>\n\
                 <code>{}</code>\n\n\
                 👉 <i>Hãy copy link trên và chia sẻ lên Facebook, Zalo hoặc nhóm săn sale nhé!</i>",
                share_link
            );

            bot.send_message(msg.chat.id, share_html)
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username))
                .await?;
        }

        Command::Stats => {
            let mut rows = conn.query("SELECT COUNT(*) FROM users", ()).await.unwrap();
            let total_users: i64 = if let Some(r) = rows.next().await.unwrap() {
                r.get(0).unwrap_or(0)
            } else {
                0
            };

            let mut rows_clicks = conn.query("SELECT COUNT(*) FROM clicks", ()).await.unwrap();
            let total_clicks: i64 = if let Some(r) = rows_clicks.next().await.unwrap() {
                r.get(0).unwrap_or(0)
            } else {
                0
            };

            let stats_html = format!(
                "📊 <b>BÁO CÁO HOẠT ĐỘNG BOT (DÀNH CHO CHỦ BOT)</b>\n\n\
                 👥 <b>Tổng số người dùng bot:</b> <b>{} người</b>\n\
                 🔗 <b>Tổng số lượt dán link & tạo mã:</b> <b>{} lượt</b>\n\n\
                 💰 <b>Hoa hồng tiếp thị:</b>\n\
                 100% doanh thu chiết khấu từ các lượt click trên sẽ được sàn TMĐT đối soát và chuyển trực tiếp vào tài khoản ngân hàng của bạn hàng tháng!",
                total_users, total_clicks
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
) -> ResponseResult<()> {
    let data = match q.data.as_deref() {
        Some(d) => d,
        None => return Ok(()),
    };

    let chat_id = match q.message {
        Some(ref msg) => msg.chat().id,
        None => return Ok(()),
    };

    let conn = match db.conn() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    let _ = bot.answer_callback_query(q.id).await;

    match data {
        "menu_deals" | "deals_all" => {
            if let Ok(deals) = Repository::get_active_deals(&conn, None).await {
                let mut text = "🔥 <b>MÃ GIẢM GIÁ TẤT CẢ CÁC SÀN</b> 🔥\n\n".to_string();
                for (i, d) in deals.iter().take(6).enumerate() {
                    let code_str = d
                        .coupon_code
                        .as_deref()
                        .map(|c| format!("Mã: <code>{}</code>", c))
                        .unwrap_or_else(|| "Tự động áp dụng".to_string());

                    text.push_str(&format!(
                        "<b>{}. [{}] {}</b>\n🏷️ {}\n🎁 {}\n👉 <a href=\"{}\">Dùng ngay</a>\n\n",
                        i + 1,
                        d.platform,
                        d.title,
                        code_str,
                        d.discount_desc,
                        d.deal_url
                    ));
                }
                bot.send_message(chat_id, text)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(deals_filter_keyboard())
                    .await?;
            }
        }

        "deals_shopee" => {
            if let Ok(deals) = Repository::get_active_deals(&conn, Some("Shopee")).await {
                let mut text = "🟠 <b>MÃ GIẢM GIÁ SHOPEE HÔM NAY</b> 🟠\n\n".to_string();
                for (i, d) in deals.iter().enumerate() {
                    text.push_str(&format!(
                        "<b>{}. {}</b>\n🏷️ Mã: <code>{}</code>\n🎁 {}\n👉 <a href=\"{}\">Lưu mã</a>\n\n",
                        i + 1,
                        d.title,
                        d.coupon_code.as_deref().unwrap_or("FREE"),
                        d.discount_desc,
                        d.deal_url
                    ));
                }
                bot.send_message(chat_id, text)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(deals_filter_keyboard())
                    .await?;
            }
        }

        "deals_lazada" => {
            if let Ok(deals) = Repository::get_active_deals(&conn, Some("Lazada")).await {
                let mut text = "🔵 <b>VOUCHER LAZADA HOT HÔM NAY</b> 🔵\n\n".to_string();
                for (i, d) in deals.iter().enumerate() {
                    text.push_str(&format!(
                        "<b>{}. {}</b>\n🏷️ Mã: <code>{}</code>\n🎁 {}\n👉 <a href=\"{}\">Lưu mã</a>\n\n",
                        i + 1,
                        d.title,
                        d.coupon_code.as_deref().unwrap_or("Tự động áp"),
                        d.discount_desc,
                        d.deal_url
                    ));
                }
                bot.send_message(chat_id, text)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(deals_filter_keyboard())
                    .await?;
            }
        }

        "deals_tiki" => {
            if let Ok(deals) = Repository::get_active_deals(&conn, Some("Tiki")).await {
                let mut text = "🔵 <b>MÃ GIẢM GIÁ TIKI HOT</b> 🔵\n\n".to_string();
                for (i, d) in deals.iter().enumerate() {
                    text.push_str(&format!(
                        "<b>{}. {}</b>\n🏷️ Mã: <code>{}</code>\n🎁 {}\n👉 <a href=\"{}\">Lưu mã</a>\n\n",
                        i + 1,
                        d.title,
                        d.coupon_code.as_deref().unwrap_or("TIKI"),
                        d.discount_desc,
                        d.deal_url
                    ));
                }
                bot.send_message(chat_id, text)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(deals_filter_keyboard())
                    .await?;
            }
        }

        "menu_guide" => {
            let guide = "📖 <b>HƯỚNG DẪN 3 BƯỚC NHẬN MÃ GIẢM GIÁ:</b>\n\n\
                         1️⃣ Copy link sản phẩm trên app Shopee, Lazada hoặc Tiki.\n\
                         2️⃣ Dán link vào khung chat này để bot gắn mã giảm giá.\n\
                         3️⃣ Bấm nút mua hàng để mở app sàn và đặt hàng với giá đã giảm!\n\n\
                         <i>Mẹo: Xóa sản phẩm cũ trong giỏ hàng trước khi mua để tránh bị lỗi mã!</i>";
            bot.send_message(chat_id, guide)
                .parse_mode(ParseMode::Html)
                .await?;
        }

        "menu_main" => {
            bot.send_message(chat_id, "🏠 <b>MENU CHÍNH BOT SĂN MÃ GIẢM GIÁ</b>")
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username))
                .await?;
        }

        _ => {}
    }

    Ok(())
}

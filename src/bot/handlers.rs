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
use crate::utils::{format_vnd, mask_account, sanitize_crlf};

#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase", description = "Danh sách lệnh hỗ trợ:")]
pub enum Command {
    #[command(description = "Bắt đầu sử dụng bot và kích hoạt")]
    Start(String),
    #[command(description = "Xem hướng dẫn chi tiết cách săn sale & hoàn tiền")]
    Help,
    #[command(description = "Kiểm tra ví hoàn tiền, số dư & hoa hồng giới thiệu")]
    Wallet,
    #[command(description = "Kiểm tra ví hoàn tiền (viết tắt tiếng Việt)")]
    Vi,
    #[command(description = "Xem mã giảm giá & deals hot (Shopee, Lazada, Tiki)")]
    Deals(String),
    #[command(description = "Lấy link giới thiệu bạn bè nhận hoa hồng trọn đời")]
    Share,
    #[command(description = "Xem lịch sử click link và đơn hoàn tiền")]
    History,
    #[command(description = "Rút tiền về tài khoản ngân hàng")]
    Withdraw(String),
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
        .unwrap_or_else(|| "Thành viên".to_string());

    // Xử lý các nút bấm từ Reply Keyboard thường trực ở đáy màn hình
    let mapped_cmd = match text {
        "🚀 Bắt Đầu" | "Bắt Đầu" | "start" => Some(Command::Start(String::new())),
        "💰 Ví Hoàn Tiền" | "Ví Hoàn Tiền" => Some(Command::Wallet),
        "🎟️ Mã Giảm Giá" | "Mã Giảm Giá" => Some(Command::Deals(String::new())),
        "🤝 Giới Thiệu Bạn Bè" | "Giới Thiệu" => Some(Command::Share),
        "📖 Hướng Dẫn" | "Hướng Dẫn" => Some(Command::Help),
        "💳 Rút Tiền" | "Rút Tiền" => Some(Command::Withdraw(String::new())),
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
        // Đảm bảo user đã tồn tại trong DB
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

        // Gửi thông báo đang xử lý
        let wait_msg = bot
            .send_message(msg.chat.id, "🔍 <i>Đang phân tích link và tạo mã hoàn tiền cá nhân...</i>")
            .parse_mode(ParseMode::Html)
            .await?;

        // Bóc tách link và tạo link hoàn tiền
        match parser
            .process_link(
                text,
                user.telegram_id,
                user.referred_by,
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

                let reply_html = format!(
                    "🎉 <b>PHÁT HIỆN DEAL SÀN {}!</b>\n\n\
                     🛒 <b>Link sạch:</b> <code>{}</code>\n\
                     💰 <b>Mức hoàn dự kiến:</b> <b>{}</b>\n\
                     👤 <b>Mã định danh hoàn tiền:</b> <code>{}</code>\n\n\
                     💡 <i>Mẹo: Bấm nút bên dưới để mở App sàn mua hàng. Tiền hoàn sẽ tự động ghi nhận vào ví của bạn trong vòng 24h - 48h sau khi đặt hàng thành công!</i>",
                    parsed.platform.as_str(),
                    parsed.clean_url,
                    parsed.estimated_cashback_text,
                    user.referral_code
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
         🤖 <b>Bot Săn Sale & Hoàn Tiền</b> hoạt động như sau:\n\
         1️⃣ Mở app <b>Shopee, Lazada, Tiki</b> và copy link món đồ bạn muốn mua.\n\
         2️⃣ Dán trực tiếp đường link vào khung chat này.\n\
         3️⃣ Bot sẽ làm sạch link và tạo nút mua kèm mã <b>Hoàn Tiền Mặt</b> về ví của bạn y như Shopback!\n\n\
         👉 Dùng lệnh /wallet để xem số dư, hoặc /deals để lấy mã giảm giá hôm nay.",
        full_name
    );

    bot.send_message(msg.chat.id, reply)
        .parse_mode(ParseMode::Html)
        .reply_markup(main_menu_keyboard(&config.bot_username, "NEW"))
        .await?;

    Ok(())
}

/// Xử lý các lệnh Slash Commands (/start, /wallet, /deals,...)
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
        .unwrap_or_else(|| "Thành viên".to_string());

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
        Command::Start(param) => {
            let ref_param = if param.trim().is_empty() {
                None
            } else {
                Some(param.trim())
            };

            let (user, is_new) = match Repository::get_or_create_user(
                &conn,
                user_id,
                username,
                &full_name,
                ref_param,
            )
            .await
            {
                Ok(res) => res,
                Err(err) => {
                    error!("Lỗi get_or_create_user: {}", err);
                    bot.send_message(msg.chat.id, "❌ Có lỗi khi khởi tạo tài khoản của bạn. Vui lòng thử lại sau.")
                        .await?;
                    return Ok(());
                }
            };

            let welcome_extra = if is_new && user.referred_by.is_some() {
                "\n🎁 <i>Bạn đã được kết nối qua mã giới thiệu của bạn bè! Bạn sẽ nhận thêm 10,000₫ vào đơn hàng hoàn tiền đầu tiên!</i>\n"
            } else {
                ""
            };

            let welcome = format!(
                "🎉 <b>CHÀO MỪNG BẠN ĐẾN VỚI BOT SĂN SALE & HOÀN TIỀN</b> 🎉\n{}\n\
                 💎 <b>Mã giới thiệu của bạn:</b> <code>{}</code>\n\
                 🔗 <b>Link giới thiệu:</b> <code>https://t.me/{}?start={}</code>\n\n\
                 📌 <b>Cách kiếm tiền & tiết kiệm:</b>\n\
                 • <b>Hoàn tiền mua sắm:</b> Dán bất kỳ link sản phẩm Shopee/Lazada/Tiki vào đây trước khi mua để nhận lại tiền mặt.\n\
                 • <b>Hoa hồng giới thiệu:</b> Mời bạn bè cùng săn sale. Khi bạn bè nhận hoàn tiền, bạn nhận thêm <b>10% hoa hồng trọn đời</b>!\n\n\
                 👇 <i>Chọn chức năng bên dưới hoặc gửi link sản phẩm ngay để thử:</i>",
                welcome_extra,
                user.referral_code,
                config.bot_username,
                user.referral_code
            );

            // Bật bàn phím chức năng thường trực dưới khung chat
            let _ = bot.send_message(msg.chat.id, "🔘 <i>Menu phím tắt nhanh đã được ghim cố định ở đáy khung chat!</i>")
                .parse_mode(ParseMode::Html)
                .reply_markup(persistent_reply_keyboard())
                .await;

            bot.send_message(msg.chat.id, welcome)
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username, &user.referral_code))
                .await?;
        }

        Command::Help => {
            let help_text = format!(
                "📖 <b>HƯỚNG DẪN SỬ DỤNG BOT HOÀN TIỀN NHƯ SHOPBACK</b>\n\n\
                 1️⃣ <b>Cách nhận hoàn tiền khi mua sắm:</b>\n\
                 • Bước 1: Vào Shopee, Lazada hoặc Tiki chọn sản phẩm bạn ưng ý.\n\
                 • Bước 2: Bấm nút <b>Chia sẻ</b> -> chọn <b>Sao chép liên kết</b>.\n\
                 • Bước 3: Dán link vào khung chat này. Bot sẽ gửi lại cho bạn nút mua hàng kèm mã hoàn tiền.\n\
                 • Bước 4: Bấm vào nút mua hàng để mở App sàn và tiến hành thanh toán.\n\
                 • Bước 5: Tiền hoàn sẽ chuyển vào trạng thái <b>Chờ duyệt</b> và chuyển sang <b>Khả dụng</b> sau khi đơn thành công.\n\n\
                 2️⃣ <b>Cách rút tiền:</b>\n\
                 • Gõ lệnh theo cú pháp: <code>/withdraw [số_tiền] [ngân_hàng] [số_tk] [tên_chủ_tk]</code>\n\
                 • Ví dụ: <code>/withdraw 100000 MBBank 0987654321 NGUYEN VAN A</code>\n\
                 • Số tiền rút tối thiểu: <b>{}</b>.\n\n\
                 3️⃣ <b>Kiếm tiền thụ động với bạn bè:</b>\n\
                 • Gõ /share để lấy link giới thiệu. Nhận ngay 10% hoa hồng trên mỗi đơn bạn bè mua thành công!",
                format_vnd(config.min_withdrawal_vnd)
            );

            bot.send_message(msg.chat.id, help_text)
                .parse_mode(ParseMode::Html)
                .await?;
        }

        Command::Wallet | Command::Vi => {
            let summary = match Repository::get_wallet_summary(&conn, user_id).await {
                Ok(s) => s,
                Err(err) => {
                    error!("Lỗi lấy ví: {}", err);
                    bot.send_message(msg.chat.id, "❌ Không thể tải thông tin ví. Vui lòng bấm /start để kích hoạt trước.")
                        .await?;
                    return Ok(());
                }
            };

            let wallet_html = format!(
                "💼 <b>VÍ HOÀN TIỀN CỦA BẠN</b>\n\n\
                 🟢 <b>Số dư khả dụng:</b> <b>{}</b> <i>(Rút được ngay)</i>\n\
                 🟡 <b>Số dư chờ duyệt:</b> <b>{}</b> <i>(Đang đối soát)</i>\n\
                 ⚪ <b>Tổng tiền đã rút:</b> <b>{}</b>\n\
                 🏆 <b>Tổng thu nhập tích lũy:</b> <b>{}</b>\n\n\
                 👥 <b>Số bạn bè đã giới thiệu (F1):</b> <b>{} người</b>\n\
                 🎁 <b>Hoa hồng từ bạn bè:</b> <b>{}</b>\n\n\
                 🔗 <b>Link giới thiệu của bạn:</b>\n\
                 <code>https://t.me/{}?start={}</code>\n\n\
                 💡 <i>Để rút tiền về tài khoản ngân hàng, dùng lệnh /withdraw</i>",
                format_vnd(summary.balance_available),
                format_vnd(summary.balance_pending),
                format_vnd(summary.balance_withdrawn),
                format_vnd(summary.total_earned),
                summary.total_referrals,
                format_vnd(summary.total_ref_bonus),
                config.bot_username,
                summary.referral_code
            );

            bot.send_message(msg.chat.id, wallet_html)
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username, &summary.referral_code))
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
            let user = Repository::get_user_by_telegram_id(&conn, user_id)
                .await
                .ok()
                .flatten();

            let ref_code = user.map(|u| u.referral_code).unwrap_or_else(|| "VIP".to_string());
            let share_link = format!("https://t.me/{}?start={}", config.bot_username, ref_code);

            let share_html = format!(
                "🤝 <b>CHƯƠNG TRÌNH GIỚI THIỆU BẠN BÈ - NHẬN HOA HỒNG TRỌN ĐỜI</b> 🤝\n\n\
                 Mỗi khi bạn bè mua sắm qua bot và nhận tiền hoàn, bạn sẽ nhận được <b>10% hoa hồng</b> trên tổng tiền hoàn của họ vĩnh viễn!\n\n\
                 💎 <b>Mã giới thiệu của bạn:</b> <code>{}</code>\n\
                 🔗 <b>Link chia sẻ trực tiếp:</b>\n\
                 <code>{}</code>\n\n\
                 👉 <i>Hãy copy link trên và gửi cho bạn bè hoặc các nhóm săn sale để xây dựng dòng thu nhập thụ động!</i>",
                ref_code, share_link
            );

            bot.send_message(msg.chat.id, share_html)
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username, &ref_code))
                .await?;
        }

        Command::History => {
            let clicks = Repository::get_recent_clicks(&conn, user_id, 5)
                .await
                .unwrap_or_default();
            let orders = Repository::get_recent_cashback_orders(&conn, user_id, 5)
                .await
                .unwrap_or_default();
            let withdrawals = Repository::get_recent_withdrawals(&conn, user_id, 5)
                .await
                .unwrap_or_default();

            let mut text = "📜 <b>LỊCH SỬ HOẠT ĐỘNG GẦN ĐÂY</b>\n\n".to_string();

            text.push_str("🛍️ <b>Đơn hoàn tiền:</b>\n");
            if orders.is_empty() {
                text.push_str("<i>Chưa có đơn hàng nào được ghi nhận. Hãy dán link và mua sắm ngay!</i>\n\n");
            } else {
                for o in orders {
                    let status_badge = match o.status.as_str() {
                        "APPROVED" => "🟢 Đã duyệt",
                        "REJECTED" => "🔴 Bị từ chối",
                        _ => "🟡 Chờ đối soát",
                    };
                    text.push_str(&format!(
                        "• Đơn <code>{}</code> ({}) - Hoàn: <b>{}</b> [{}]\n",
                        o.order_id,
                        o.platform,
                        format_vnd(o.user_cashback),
                        status_badge
                    ));
                }
                text.push('\n');
            }

            text.push_str("💳 <b>Lịch sử rút tiền:</b>\n");
            if withdrawals.is_empty() {
                text.push_str("<i>Chưa có yêu cầu rút tiền nào.</i>\n\n");
            } else {
                for w in withdrawals {
                    let st = match w.status.as_str() {
                        "COMPLETED" => "🟢 Thành công",
                        "REJECTED" => "🔴 Bị từ chối",
                        _ => "🟡 Đang xử lý",
                    };
                    text.push_str(&format!(
                        "• Mã <code>{}</code>: <b>{}</b> về {} [{}]\n",
                        w.id,
                        format_vnd(w.amount),
                        w.bank_name,
                        st
                    ));
                }
                text.push('\n');
            }

            text.push_str("🔗 <b>5 lượt tạo link gần nhất:</b>\n");
            if clicks.is_empty() {
                text.push_str("<i>Chưa có lượt tạo link nào.</i>\n");
            } else {
                for c in clicks {
                    text.push_str(&format!(
                        "• [{}] {}\n",
                        c.platform,
                        c.clean_url
                    ));
                }
            }

            bot.send_message(msg.chat.id, text)
                .parse_mode(ParseMode::Html)
                .await?;
        }

        Command::Withdraw(params_str) => {
            let parts: Vec<&str> = params_str.split_whitespace().collect();
            if parts.len() < 4 {
                let guide = format!(
                    "⚠️ <b>Cú pháp rút tiền chưa chính xác!</b>\n\n\
                     Vui lòng nhập theo định dạng:\n\
                     <code>/withdraw [số_tiền] [ngân_hàng] [số_tài_khoản] [tên_chủ_tài_khoản]</code>\n\n\
                     <b>Ví dụ mẫu:</b>\n\
                     <code>/withdraw 100000 MBBank 0987654321 NGUYEN VAN A</code>\n\n\
                     📌 <i>Mức rút tối thiểu: {}</i>",
                    format_vnd(config.min_withdrawal_vnd)
                );
                bot.send_message(msg.chat.id, guide)
                    .parse_mode(ParseMode::Html)
                    .await?;
                return Ok(());
            }

            let amount: f64 = match parts[0].replace([',', '.'], "").parse() {
                Ok(a) => a,
                Err(_) => {
                    bot.send_message(msg.chat.id, "❌ Số tiền rút không hợp lệ. Vui lòng nhập số nguyên.")
                        .await?;
                    return Ok(());
                }
            };

            let bank_name = sanitize_crlf(parts[1]);
            let account_number = sanitize_crlf(parts[2]);
            let account_name = sanitize_crlf(&parts[3..].join(" "));

            match Repository::create_withdrawal(
                &conn,
                user_id,
                amount,
                &bank_name,
                &account_number,
                &account_name,
                config.min_withdrawal_vnd,
            )
            .await
            {
                Ok(wd) => {
                    let success_html = format!(
                        "✅ <b>YÊU CẦU RÚT TIỀN ĐÃ ĐƯỢC TIẾP NHẬN</b>\n\n\
                         🆔 <b>Mã giao dịch:</b> <code>{}</code>\n\
                         💵 <b>Số tiền:</b> <b>{}</b>\n\
                         🏦 <b>Ngân hàng:</b> {}\n\
                         🔢 <b>Số tài khoản:</b> {}\n\
                         👤 <b>Tên người nhận:</b> {}\n\
                         ⏳ <b>Trạng thái:</b> 🟡 Đang xử lý\n\n\
                         <i>Hệ thống sẽ chuyển khoản cho bạn trong vòng 1-2 ngày làm việc. Cảm ơn bạn đã đồng hành!</i>",
                        wd.id,
                        format_vnd(wd.amount),
                        wd.bank_name,
                        mask_account(&wd.account_number),
                        wd.account_name
                    );

                    bot.send_message(msg.chat.id, success_html)
                        .parse_mode(ParseMode::Html)
                        .await?;
                }
                Err(err) => {
                    let err_html = format!("❌ <b>Không thể tạo yêu cầu rút tiền:</b>\n\n{}", err);
                    bot.send_message(msg.chat.id, err_html)
                        .parse_mode(ParseMode::Html)
                        .await?;
                }
            }
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

    let user_id = q.from.id.0 as i64;
    let chat_id = match q.message {
        Some(ref msg) => msg.chat().id,
        None => return Ok(()),
    };

    let conn = match db.conn() {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    // Xác nhận đã nhận callback với Telegram
    let _ = bot.answer_callback_query(q.id).await;

    match data {
        "menu_wallet" => {
            if let Ok(summary) = Repository::get_wallet_summary(&conn, user_id).await {
                let wallet_html = format!(
                    "💼 <b>VÍ HOÀN TIỀN CỦA BẠN</b>\n\n\
                     🟢 <b>Số dư khả dụng:</b> <b>{}</b>\n\
                     🟡 <b>Số dư chờ duyệt:</b> <b>{}</b>\n\
                     ⚪ <b>Tổng đã rút:</b> <b>{}</b>\n\
                     👥 <b>Bạn bè giới thiệu:</b> <b>{} người</b>\n\
                     🎁 <b>Hoa hồng bạn bè:</b> <b>{}</b>\n\n\
                     Mã giới thiệu: <code>{}</code>\n\
                     Link: <code>https://t.me/{}?start={}</code>",
                    format_vnd(summary.balance_available),
                    format_vnd(summary.balance_pending),
                    format_vnd(summary.balance_withdrawn),
                    summary.total_referrals,
                    format_vnd(summary.total_ref_bonus),
                    summary.referral_code,
                    config.bot_username,
                    summary.referral_code
                );
                bot.send_message(chat_id, wallet_html)
                    .parse_mode(ParseMode::Html)
                    .reply_markup(main_menu_keyboard(&config.bot_username, &summary.referral_code))
                    .await?;
            }
        }

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

        "menu_history" => {
            let orders = Repository::get_recent_cashback_orders(&conn, user_id, 5)
                .await
                .unwrap_or_default();
            let mut text = "📜 <b>LỊCH SỬ ĐƠN HOÀN TIỀN</b>\n\n".to_string();
            if orders.is_empty() {
                text.push_str("<i>Chưa có đơn hoàn tiền nào. Hãy dán link sản phẩm và đặt hàng ngay để nhận tiền hoàn nhé!</i>");
            } else {
                for o in orders {
                    text.push_str(&format!(
                        "• Đơn <code>{}</code> - Hoàn: <b>{}</b> ({})\n",
                        o.order_id,
                        format_vnd(o.user_cashback),
                        o.status
                    ));
                }
            }
            bot.send_message(chat_id, text)
                .parse_mode(ParseMode::Html)
                .await?;
        }

        "menu_guide" => {
            let guide = "📖 <b>HƯỚNG DẪN 3 BƯỚC NHẬN HOÀN TIỀN:</b>\n\n\
                         1️⃣ Copy link sản phẩm trên app Shopee, Lazada hoặc Tiki.\n\
                         2️⃣ Dán link vào khung chat này để bot tạo link hoàn tiền.\n\
                         3️⃣ Bấm nút mua hàng để mở app sàn và đặt hàng bình thường.\n\n\
                         <i>Sau khi nhận hàng thành công, tiền sẽ chuyển vào ví hoàn tiền của bạn!</i>";
            bot.send_message(chat_id, guide)
                .parse_mode(ParseMode::Html)
                .await?;
        }

        "menu_withdraw_guide" => {
            let guide = format!(
                "💳 <b>HƯỚNG DẪN RÚT TIỀN VỀ TÀI KHOẢN NGÂN HÀNG:</b>\n\n\
                 Dùng lệnh: <code>/withdraw [số_tiền] [ngân_hàng] [số_tk] [tên_chủ_tk]</code>\n\n\
                 Ví dụ:\n\
                 <code>/withdraw 100000 Vietcombank 1234567890 NGUYEN VAN A</code>\n\n\
                 • Mức rút tối thiểu: <b>{}</b>\n\
                 • Tiền sẽ về tài khoản trong 24h - 48h làm việc.",
                format_vnd(config.min_withdrawal_vnd)
            );
            bot.send_message(chat_id, guide)
                .parse_mode(ParseMode::Html)
                .await?;
        }

        "menu_main" => {
            let user = Repository::get_user_by_telegram_id(&conn, user_id)
                .await
                .ok()
                .flatten();
            let ref_code = user.map(|u| u.referral_code).unwrap_or_else(|| "NEW".to_string());
            bot.send_message(chat_id, "🏠 <b>MENU CHÍNH BOT SĂN SALE</b>")
                .parse_mode(ParseMode::Html)
                .reply_markup(main_menu_keyboard(&config.bot_username, &ref_code))
                .await?;
        }

        _ => {}
    }

    Ok(())
}

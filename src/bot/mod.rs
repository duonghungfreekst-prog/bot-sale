pub mod handlers;
pub mod keyboards;

use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::BotCommand;
use tracing::{info, warn};

use crate::config::Config;
use crate::db::AppDb;
use crate::services::deal_parser::DealParser;

pub async fn run_bot(config: Config, db: AppDb) -> anyhow::Result<()> {
    info!("Khởi tạo Telegram Bot: @{}...", config.bot_username);
    let bot = Bot::new(&config.telegram_token);
    let parser = Arc::new(DealParser::new());

    // Tự động đăng ký danh sách commands để Telegram hiện nút [Menu] cạnh ô nhập tin nhắn
    let commands = vec![
        BotCommand::new("start", "🚀 Bắt đầu / Khởi động lại bot"),
        BotCommand::new("wallet", "💰 Xem ví & số dư hoàn tiền"),
        BotCommand::new("deals", "🎟️ Xem mã giảm giá hot hôm nay"),
        BotCommand::new("share", "🤝 Mời bạn bè nhận hoa hồng 10%"),
        BotCommand::new("history", "📜 Lịch sử đơn & lượt click"),
        BotCommand::new("withdraw", "💳 Rút tiền về tài khoản ngân hàng"),
        BotCommand::new("help", "📖 Hướng dẫn chi tiết cách săn sale"),
    ];

    if let Err(e) = bot.set_my_commands(commands).await {
        warn!("Không thể tự động đăng ký commands với Telegram: {}", e);
    } else {
        info!("✅ Đã đăng ký thành công Menu Commands với Telegram API!");
    }

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(handlers::handle_message))
        .branch(Update::filter_callback_query().endpoint(handlers::handle_callback));

    info!("Bot đã sẵn sàng lắng nghe tin nhắn và sự kiện!");
    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![db, config, parser])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

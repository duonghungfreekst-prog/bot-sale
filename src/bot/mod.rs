pub mod handlers;
pub mod keyboards;

use std::sync::Arc;
use teloxide::prelude::*;
use tracing::info;

use crate::config::Config;
use crate::db::AppDb;
use crate::services::deal_parser::DealParser;

pub async fn run_bot(config: Config, db: AppDb) -> anyhow::Result<()> {
    info!("Khởi tạo Telegram Bot: @{}...", config.bot_username);
    let bot = Bot::new(&config.telegram_token);
    let parser = Arc::new(DealParser::new());

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

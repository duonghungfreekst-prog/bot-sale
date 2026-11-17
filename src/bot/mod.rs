pub mod handlers;
pub mod keyboards;

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use teloxide::error_handlers::LoggingErrorHandler;
use teloxide::prelude::*;
use teloxide::requests::Requester;
use teloxide::stop::{mk_stop_token, StopToken};
use teloxide::types::{BotCommand, Update};
use teloxide::update_listeners::StatefulListener;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{error, info, warn};
use url::Url;

use crate::config::Config;
use crate::db::AppDb;
use crate::services::accesstrade::AccessTradeClient;
use crate::services::deal_parser::DealParser;

pub type WebhookUpdateReceiver = mpsc::UnboundedReceiver<Result<Update, Infallible>>;

/// Khởi chạy Telegram Bot (tự động chọn Webhook 24/7 trên Cloud hoặc Long Polling ở Local)
pub async fn run_bot(
    config: Config,
    db: AppDb,
    accesstrade: Arc<AccessTradeClient>,
    webhook_rx: Option<WebhookUpdateReceiver>,
    web_ready_rx: Option<oneshot::Receiver<()>>,
) -> anyhow::Result<()> {
    info!("Khởi tạo Telegram Bot: @{}...", config.bot_username);
    let bot = Bot::new(&config.telegram_token);
    let parser = Arc::new(DealParser::with_accesstrade(accesstrade));

    // Tự động đăng ký danh sách commands để Telegram hiện nút [Menu] cạnh ô nhập tin nhắn
    let commands = vec![
        BotCommand::new("start", "🚀 Bắt đầu / Dán link sản phẩm nhận ưu đãi"),
        BotCommand::new("help", "📖 Mẹo áp mã giảm giá thành công"),
    ];

    if let Err(e) = bot.set_my_commands(commands).await {
        warn!("Không thể tự động đăng ký commands với Telegram: {}", e);
    } else {
        info!("✅ Đã đăng ký thành công Menu Commands với Telegram API!");
    }

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(handlers::handle_message))
        .branch(Update::filter_callback_query().endpoint(handlers::handle_callback));

    if let (Some(base_url), Some(rx)) = (config.public_base_url.clone(), webhook_rx) {
        if let Some(ready) = web_ready_rx {
            let _ = tokio::time::timeout(Duration::from_secs(15), ready).await;
        }

        let webhook_endpoint = format!("{}/telegram/webhook", base_url);
        match setup_webhook_with_retry(&bot, &webhook_endpoint, &config.telegram_webhook_secret).await {
            Ok(()) => {
                info!("🌐 Bot đang chạy ở chế độ Webhook 24/7 tại: {}", webhook_endpoint);
                let (stop_token, _stop_flag) = mk_stop_token();
                let stream = UnboundedReceiverStream::new(rx);
                let listener = StatefulListener::new(
                    (stream, stop_token),
                    tuple_first_mut,
                    |state: &mut (_, StopToken)| state.1.clone(),
                );

                let mut dispatcher = Dispatcher::builder(bot, handler)
                    .dependencies(dptree::deps![db, config, parser])
                    .enable_ctrlc_handler()
                    .build();

                dispatcher
                    .dispatch_with_listener(
                        listener,
                        LoggingErrorHandler::with_custom_text("Webhook listener error"),
                    )
                    .await;
                return Ok(());
            }
            Err(err) => {
                error!(
                    "⚠️ Không thể kích hoạt Webhook ({}): {} — tự động chuyển về Long Polling.",
                    webhook_endpoint, err
                );
            }
        }
    }

    // Chế độ Long Polling (Local hoặc fallback khi Webhook chưa sẵn sàng)
    if let Err(err) = bot.delete_webhook().drop_pending_updates(false).send().await {
        warn!("Không thể gỡ Webhook cũ trước khi bật Long Polling: {}", err);
    }

    info!("Bot đã sẵn sàng lắng nghe tin nhắn qua Long Polling!");
    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![db, config, parser])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

/// Đăng ký Webhook với Telegram API (thử lại tối đa 3 lần khi mạng Cloud mới khởi động)
async fn setup_webhook_with_retry(
    bot: &Bot,
    webhook_endpoint: &str,
    secret_token: &str,
) -> anyhow::Result<()> {
    let parsed_url = Url::parse(webhook_endpoint)?;
    let mut last_err = None;

    for attempt in 1..=3 {
        let res = bot
            .set_webhook(parsed_url.clone())
            .secret_token(secret_token.to_string())
            .drop_pending_updates(false)
            .send()
            .await;

        match res {
            Ok(_) => return Ok(()),
            Err(err) => {
                warn!(
                    "Lần {}/3 đăng ký Telegram Webhook thất bại: {}",
                    attempt, err
                );
                last_err = Some(err);
                if attempt < 3 {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    Err(anyhow::anyhow!(
        "SetWebhook thất bại sau 3 lần thử: {:?}",
        last_err
    ))
}

fn tuple_first_mut<A, B>(tuple: &mut (A, B)) -> &mut A {
    &mut tuple.0
}

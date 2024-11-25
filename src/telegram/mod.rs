use std::env;

use color_eyre::eyre::Context;
use teloxide::prelude::*;
use tracing::*;

use crate::broadcast::{broadcast, Source};

mod parsers;
use self::parsers::*;

#[instrument(skip(bot, m))]
async fn message_handle(bot: Bot, m: Message, chat_id: i64) -> color_eyre::Result<()> {
    if m.chat.id.0 != chat_id {
        return Ok(());
    }

    let core_message = to_core_message(bot, m).await?;
    debug!(?core_message, "parsed core message");

    broadcast(&core_message, Source::Telegram).await?;

    Ok(())
}

#[instrument]
pub async fn start() {
    info!("setting up telegram bot");
    let bot = Bot::from_env();
    let chat_id = env::var("TELEGRAM_CHAT_ID")
        .context("Set the TELEGRAM_CHAT_ID environment variable to a chat ID you want to bridge.")
        .expect("Could not get chat ID")
        .parse::<i64>()
        .expect("Chat ID must be an i64");
    let handler = dptree::entry().branch(Update::filter_message().endpoint(message_handle));
    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![chat_id])
        .build()
        .dispatch()
        .await;
}

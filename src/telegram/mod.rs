use std::{env, sync::Arc};

use color_eyre::eyre::Context;
use teloxide::prelude::*;
use tokio::sync::Mutex;
use tracing::*;

use crate::broadcast::{Broadcaster, Source};

mod broadcast;
mod entities;
mod parsers;
use self::parsers::*;

pub struct TelegramBridge {
    pub bot: Bot,
    pub chat_id: i64,

    broadcaster: Arc<Mutex<Broadcaster>>,
}

impl TelegramBridge {
    pub fn init(broadcaster: Arc<Mutex<Broadcaster>>) -> TelegramBridge {
        info!("setting up telegram bot");

        let bot = Bot::from_env();
        let chat_id = env::var("TELEGRAM_CHAT_ID")
            .context(
                "Set the TELEGRAM_CHAT_ID environment variable to a chat ID you want to bridge.",
            )
            .expect("Could not get chat ID")
            .parse::<i64>()
            .expect("Chat ID must be an i64");

        TelegramBridge {
            bot,
            chat_id,
            broadcaster,
        }
    }

    pub async fn start(&self) {
        let handler = dptree::entry().branch(Update::filter_message().endpoint(message_handle));
        Dispatcher::builder(self.bot.clone(), handler)
            .dependencies(dptree::deps![self.chat_id, self.broadcaster.clone()])
            .build()
            .dispatch()
            .await;
    }
}

async fn message_handle(
    bot: Bot,
    message: Message,
    chat_id: i64,
    broadcaster: Arc<Mutex<Broadcaster>>,
) -> color_eyre::Result<()> {
    if message.chat.id.0 != chat_id {
        return Ok(());
    }

    let core_message = to_core_message(bot, message).await?;
    debug!(?core_message, "parsed core message");

    broadcaster
        .lock()
        .await
        .broadcast(&core_message, Source::Telegram)
        .await?;

    Ok(())
}

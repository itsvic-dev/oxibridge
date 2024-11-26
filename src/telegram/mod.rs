use std::sync::Arc;
use teloxide::prelude::*;
use tokio::sync::Mutex;
use tracing::*;

use crate::{
    broadcast::{Broadcaster, Source},
    config::GroupConfig,
    Config,
};

mod broadcast;
mod entities;
mod parsers;
use self::parsers::*;

pub struct TelegramBridge {
    pub bot: Bot,

    broadcaster: Arc<Mutex<Broadcaster>>,
    config: Arc<Config>,
}

impl TelegramBridge {
    pub fn init(broadcaster: Arc<Mutex<Broadcaster>>, config: Arc<Config>) -> TelegramBridge {
        let bot = Bot::new(&config.shared.telegram_token);

        TelegramBridge {
            bot,
            broadcaster,
            config,
        }
    }

    pub async fn start(&self) {
        let handler = dptree::entry().branch(Update::filter_message().endpoint(message_handle));
        Dispatcher::builder(self.bot.clone(), handler)
            .dependencies(dptree::deps![self.config.clone(), self.broadcaster.clone()])
            .build()
            .dispatch()
            .await;
    }
}

async fn message_handle(
    bot: Bot,
    message: Message,
    config: Arc<Config>,
    broadcaster: Arc<Mutex<Broadcaster>>,
) -> color_eyre::Result<()> {
    // find the respective group
    let group: Vec<GroupConfig> = config
        .groups
        .clone()
        .into_iter()
        .filter(|g| g.telegram_chat == message.chat.id.0)
        .collect();

    if group.is_empty() {
        return Ok(());
    }
    let group = group.first().unwrap();

    let core_message = to_core_message(bot, message).await?;
    debug!(?core_message, "parsed core message");

    broadcaster
        .lock()
        .await
        .broadcast(group, &core_message, Source::Telegram)
        .await?;

    Ok(())
}

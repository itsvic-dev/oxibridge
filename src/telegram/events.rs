use std::sync::Arc;

use color_eyre::eyre::eyre;
use teloxide::{types::Message, Bot};
use tokio::sync::Mutex;
use tracing::*;

use crate::{
    broadcast::{Broadcaster, MessageEvent, Source},
    config::GroupConfig,
    telegram::to_core_message,
    Config,
};

use super::TgCache;

#[instrument(skip_all)]
pub async fn message_handle(
    bot: Bot,
    message: Message,
    config: Arc<Config>,
    cache: Arc<Mutex<TgCache>>,
    broadcaster: Arc<Mutex<Broadcaster>>,
) -> color_eyre::Result<()> {
    // find the respective group
    let group: Vec<GroupConfig> = config
        .groups
        .clone()
        .into_iter()
        .filter(|g| g.telegram_chat == Some(message.chat.id.0))
        .collect();

    let group = match group.first() {
        Some(group) => group,
        None => return Ok(()),
    };

    // look up reply in cache
    let cached_reply = match message.reply_to_message() {
        Some(msg) => cache.lock().await.tg_core_cache.get(&msg.id).cloned(),
        None => None,
    };

    // split the Option<tuple> into separate Options
    let (reply_id, reply_author) = match cached_reply {
        Some((id, author)) => (Some(id), Some(author)),
        None => (None, None),
    };

    let core_message = to_core_message(bot, &message, reply_id, reply_author).await?;

    {
        let mut cache = cache.lock().await;
        cache
            .tg_core_cache
            .insert(message.id, (core_message.id, (&core_message.author).into()));
        cache
            .core_tg_cache
            .insert(core_message.id, (message.id, String::new()));
    }

    broadcaster
        .lock()
        .await
        .broadcast(group, &MessageEvent::Create(core_message), Source::Telegram)
        .await?;

    Ok(())
}

#[instrument(skip_all)]
pub async fn message_edit_handle(
    message: Message,
    config: Arc<Config>,
    cache: Arc<Mutex<TgCache>>,
    broadcaster: Arc<Mutex<Broadcaster>>,
) -> color_eyre::Result<()> {
    // find the respective group
    let group: Vec<GroupConfig> = config
        .groups
        .clone()
        .into_iter()
        .filter(|g| g.telegram_chat == Some(message.chat.id.0))
        .collect();

    let group = match group.first() {
        Some(group) => group,
        None => return Ok(()),
    };

    debug!(?group, "got group");

    // get tg->core id
    let core_id = match cache.lock().await.tg_core_cache.get(&message.id) {
        Some(id) => id.0,
        None => return Err(eyre!("failed to get core ID from {:?}", message.id)),
    };

    let text = message.text().unwrap_or(message.caption().unwrap_or(""));

    broadcaster
        .lock()
        .await
        .broadcast(
            group,
            &MessageEvent::Update(core_id, text.to_owned()),
            Source::Telegram,
        )
        .await?;

    Ok(())
}

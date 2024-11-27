use color_eyre::eyre::eyre;
use std::{collections::HashMap, sync::Arc};
use teloxide::{prelude::*, types::MessageId};
use tokio::sync::Mutex;
use tracing::*;

use crate::{
    broadcast::{Broadcaster, MessageEvent, Source},
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

    cache: Arc<Mutex<TgCache>>,
}

#[derive(Debug)]
struct TgCache {
    /// Cache of Telegram IDs to core message IDs.
    tg_core_cache: HashMap<MessageId, u64>,

    /// Cache of core message IDs to (Telegram IDs, author names).
    core_tg_cache: HashMap<u64, (MessageId, String)>,
}

impl TelegramBridge {
    #[instrument(skip_all)]
    pub fn init(broadcaster: Arc<Mutex<Broadcaster>>, config: Arc<Config>) -> TelegramBridge {
        debug!("Creating Telegram bot");
        let bot = Bot::new(&config.shared.telegram_token);

        TelegramBridge {
            bot,
            broadcaster,
            config,

            cache: Arc::new(Mutex::new(TgCache {
                core_tg_cache: HashMap::new(),
                tg_core_cache: HashMap::new(),
            })),
        }
    }

    pub async fn start(&self) {
        let handler = dptree::entry()
            .branch(Update::filter_message().endpoint(message_handle))
            .branch(Update::filter_edited_message().endpoint(message_edit_handle));
        Dispatcher::builder(self.bot.clone(), handler)
            .dependencies(dptree::deps![
                self.config.clone(),
                self.broadcaster.clone(),
                self.cache.clone()
            ])
            .build()
            .dispatch()
            .await;
    }
}

#[instrument(skip_all)]
async fn message_handle(
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
        .filter(|g| g.telegram_chat == message.chat.id.0)
        .collect();

    let group = match group.first() {
        Some(group) => group,
        None => return Ok(()),
    };

    let core_message = to_core_message(bot, &message).await?;
    debug!(?core_message, "parsed core message");

    // cache tg->core
    cache
        .lock()
        .await
        .tg_core_cache
        .insert(message.id, core_message.id);

    broadcaster
        .lock()
        .await
        .broadcast(group, &MessageEvent::Create(core_message), Source::Telegram)
        .await?;

    Ok(())
}

#[instrument(skip_all)]
async fn message_edit_handle(
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
        .filter(|g| g.telegram_chat == message.chat.id.0)
        .collect();

    let group = match group.first() {
        Some(group) => group,
        None => return Ok(()),
    };

    debug!(?group, "got group");

    // get tg->core id
    let core_id = match cache.lock().await.tg_core_cache.get(&message.id) {
        Some(id) => *id,
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

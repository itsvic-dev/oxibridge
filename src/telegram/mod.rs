use std::{collections::HashMap, sync::Arc};
use teloxide::{prelude::*, types::MessageId};
use tokio::sync::Mutex;
use tracing::*;

use crate::core::Author;
use crate::{broadcast::Broadcaster, Config};

mod broadcast;
mod entities;
mod events;
mod parsers;
mod unparse;
use self::events::*;
use self::parsers::*;

pub struct TelegramBridge {
    pub bot: Bot,

    broadcaster: Arc<Mutex<Broadcaster>>,
    config: Arc<Config>,

    cache: Arc<Mutex<TgCache>>,
}

#[derive(Debug)]
struct TgCache {
    /// Cache of Telegram IDs to (core message IDs, core authors).
    tg_core_cache: HashMap<MessageId, (u64, Arc<Author>)>,

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

use std::{collections::HashMap, sync::Arc};

use crate::{broadcast::Broadcaster, core::Author, storage::R2Storage, Config};
use color_eyre::Result;
use serenity::{
    all::{Http, MessageId},
    prelude::*,
};
use tracing::*;

mod broadcast;
mod events;
mod parsers;

pub struct DiscordBridge {
    storage: Option<Arc<Mutex<R2Storage>>>,

    /// The underlying Discord client. Will likely be locked after `start()`, so don't try to read it.
    client: Arc<Mutex<Client>>,

    /// A copy of the HTTP client for use by other parts of the app.
    http: Arc<Http>,

    cache: Arc<Mutex<DscCache>>,
}

#[derive(Debug)]
struct DscCache {
    /// Cache of Discord IDs to (core message IDs, core authors).
    dsc_core_cache: HashMap<MessageId, (u64, Arc<Author>)>,

    /// Cache of core message IDs to (Discord IDs, message headers).
    core_dsc_cache: HashMap<u64, (MessageId, String)>,
}

impl DiscordBridge {
    #[instrument(skip_all)]
    pub async fn new(
        config: Arc<Config>,
        broadcaster: Arc<Mutex<Broadcaster>>,
        storage: Option<Arc<Mutex<R2Storage>>>,
    ) -> Result<Self> {
        debug!("Creating Discord bot");

        let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

        let cache = Arc::new(Mutex::new(DscCache {
            dsc_core_cache: HashMap::new(),
            core_dsc_cache: HashMap::new(),
        }));

        let handler = BotEventHandler {
            config: config.clone(),
            broadcaster,
            cache: cache.clone(),
            http: Http::new(&config.shared.discord_token),
        };

        let client = Arc::new(Mutex::new(
            Client::builder(&config.shared.discord_token, intents)
                .event_handler(handler)
                .await?,
        ));

        Ok(DiscordBridge {
            http: client.clone().lock().await.http.clone(),
            client,
            storage,
            cache,
        })
    }

    pub async fn start(&self) {
        self.client
            .lock()
            .await
            .start()
            .await
            .expect("Client failed to start");
    }
}

struct BotEventHandler {
    broadcaster: Arc<Mutex<Broadcaster>>,
    config: Arc<Config>,

    cache: Arc<Mutex<DscCache>>,
    http: Http,
}

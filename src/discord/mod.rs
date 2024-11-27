use std::sync::Arc;

use crate::{
    broadcast::{Broadcaster, Source},
    config::GroupConfig,
    storage::R2Storage,
    Config,
};
use color_eyre::Result;
use parsers::to_core_message;
use serenity::{async_trait, model::channel::Message, prelude::*};
use tracing::*;

mod broadcast;
mod parsers;

pub struct DiscordBridge {
    storage: Option<Arc<Mutex<R2Storage>>>,

    /// The underlying Discord client. Will likely be locked after `start()`, so don't try to read it.
    client: Arc<Mutex<Client>>,
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

        let handler = BotEventHandler {
            config: config.clone(),
            broadcaster,
        };

        let client = Arc::new(Mutex::new(
            Client::builder(&config.shared.discord_token, intents)
                .event_handler(handler)
                .await?,
        ));

        Ok(DiscordBridge { client, storage })
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
    pub broadcaster: Arc<Mutex<Broadcaster>>,
    pub config: Arc<Config>,
}

#[async_trait]
impl EventHandler for BotEventHandler {
    #[instrument(skip_all)]
    async fn message(&self, _ctx: Context, msg: Message) {
        if msg.author.bot || msg.author.system {
            return;
        }

        // find the respective group
        let group: Vec<GroupConfig> = self
            .config
            .groups
            .clone()
            .into_iter()
            .filter(|g| g.discord_channel == msg.channel_id.get())
            .collect();

        let group = match group.first() {
            Some(group) => group,
            None => return,
        };

        let core_msg = match to_core_message(&msg).await {
            Ok(core_msg) => core_msg,
            Err(why) => {
                error!(?why, "Failed to parse into core message");
                return;
            }
        };

        if let Err(why) = self
            .broadcaster
            .lock()
            .await
            .broadcast(group, &core_msg, Source::Discord)
            .await
        {
            error!(?why, "Failed to broadcast message");
        }
    }
}

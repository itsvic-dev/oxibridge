use std::sync::Arc;

use crate::{
    broadcast::{Broadcaster, Source},
    config::GroupConfig,
    Config,
};
use parsers::to_core_message;
use serenity::{async_trait, model::channel::Message, prelude::*};
use tracing::*;

mod broadcast;
mod parsers;
pub use broadcast::DiscordBroadcastReceiver;

struct Handler {
    pub broadcaster: Arc<Mutex<Broadcaster>>,
    pub config: Arc<Config>,
}

#[async_trait]
impl EventHandler for Handler {
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

        if group.is_empty() {
            return;
        }
        let group = group.first().unwrap();

        let core_msg = to_core_message(&msg)
            .await
            .expect("failed to parse to core message");

        self.broadcaster
            .lock()
            .await
            .broadcast(group, &core_msg, Source::Discord)
            .await
            .expect("failed to broadcast message");
    }
}

#[instrument(skip(broadcaster))]
pub async fn start(broadcaster: Arc<Mutex<Broadcaster>>, config: Arc<Config>) {
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    let handler = Handler {
        broadcaster,
        config: config.clone(),
    };

    let mut client = Client::builder(&config.shared.discord_token, intents)
        .event_handler(handler)
        .await
        .expect("Error creating client");

    if let Err(why) = client.start().await {
        error!("Client error: {why:?}");
    };
}

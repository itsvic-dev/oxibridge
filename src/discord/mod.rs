use std::{env, sync::Arc};

use crate::broadcast::{Broadcaster, Source};
use color_eyre::eyre::Context;
use parsers::to_core_message;
use serenity::{async_trait, model::channel::Message, prelude::*};
use tracing::*;

mod broadcast;
mod parsers;
pub use broadcast::DiscordBroadcastReceiver;

struct Handler {
    pub broadcaster: Arc<Mutex<Broadcaster>>,
    pub channel_id: u64,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _ctx: serenity::prelude::Context, msg: Message) {
        if msg.channel_id.get() != self.channel_id {
            return;
        }

        if msg.author.bot || msg.author.system {
            return;
        }

        let core_msg = to_core_message(&msg)
            .await
            .expect("failed to parse to core message");

        self.broadcaster
            .lock()
            .await
            .broadcast(&core_msg, Source::Discord)
            .await
            .expect("failed to broadcast message");
    }
}

#[instrument(skip(broadcaster))]
pub async fn start(broadcaster: Arc<Mutex<Broadcaster>>) {
    let token = env::var("DISCORD_TOKEN")
        .context("Set the DISCORD_TOKEN environment variable to your bot's Discord token.")
        .expect("Could not get Discord bot token");

    let channel_id = env::var("DISCORD_CHANNEL_ID")
        .context("Set the DISCORD_CHANNEL_ID environment variable to the desired channel.")
        .expect("Could not get Discord channel ID")
        .parse::<u64>()
        .expect("Channel ID is not a u64");

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    let handler = Handler {
        broadcaster,
        channel_id,
    };

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .expect("Error creating client");

    if let Err(why) = client.start().await {
        error!("Client error: {why:?}");
    };
}

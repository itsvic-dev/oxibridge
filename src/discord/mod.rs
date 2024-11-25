use std::env;

use color_eyre::eyre::Context;
use serenity::{async_trait, model::channel::Message, prelude::*};
use tracing::*;

mod broadcast;
pub use broadcast::broadcast_message;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _ctx: serenity::prelude::Context, msg: Message) {
        debug!("got a message: {msg:?}");
    }
}

#[instrument]
pub async fn start() {
    let token = env::var("DISCORD_TOKEN")
        .context("Set the DISCORD_TOKEN environment variable to your bot's Discord token.")
        .expect("Could not get Discord bot token");

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .await
        .expect("Error creating client");

    if let Err(why) = client.start().await {
        error!("Client error: {why:?}");
    };
}

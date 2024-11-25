use std::env;

use color_eyre::eyre::{Context, Result};
use serenity::{
    async_trait,
    builder::ExecuteWebhook,
    http::Http,
    model::{channel::Message, webhook::Webhook},
    prelude::*,
};
use tracing::*;

use crate::core;

struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: serenity::prelude::Context, msg: Message) {
        debug!("got a message: {msg:?}");
    }
}

pub async fn broadcast_message(message: &core::Message) -> Result<()> {
    let http = Http::new("");
    let webhook_url =
        env::var("DISCORD_WEBHOOK").context("Set the DISCORD_WEBHOOK environment variable.")?;
    let webhook = Webhook::from_url(&http, &webhook_url).await?;

    let builder = ExecuteWebhook::new()
        .content(&message.content)
        .username(format!(
            "{} ({})",
            &message.author.display_name, &message.author.username
        ));

    webhook.execute(&http, false, builder).await?;

    Ok(())
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

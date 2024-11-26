use std::env;

use crate::{
    broadcast::{BroadcastReceiver, Source},
    core,
};
use color_eyre::eyre::{Context, Result};
use serenity::{
    all::{CreateAttachment, ExecuteWebhook, Http, Webhook},
    async_trait,
};

pub struct DiscordBroadcastReceiver;

#[async_trait]
impl BroadcastReceiver for DiscordBroadcastReceiver {
    async fn receive(&self, message: &core::Message) -> Result<()> {
        // might be useful to move to an init function
        let http = Http::new("");
        let webhook_url =
            env::var("DISCORD_WEBHOOK").context("Set the DISCORD_WEBHOOK environment variable.")?;
        let webhook = Webhook::from_url(&http, &webhook_url).await?;

        let builder = ExecuteWebhook::new()
            .content(&message.content)
            .username(message.author.full_name());

        let mut attachments: Vec<CreateAttachment> = vec![];
        for file in &message.attachments {
            let attachment_old = CreateAttachment::path(&file.file.path).await?;
            let attachment = if file.spoilered {
                CreateAttachment::bytes(
                    attachment_old.data,
                    format!("SPOILER_{}", &file.file.name).to_owned(),
                )
            } else {
                attachment_old
            };
            attachments.push(attachment);
        }

        let builder = builder.add_files(attachments);

        webhook.execute(&http, false, builder).await?;

        Ok(())
    }

    fn get_receiver_source(&self) -> Source {
        Source::Discord
    }
}

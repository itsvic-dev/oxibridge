use crate::{
    broadcast::{BroadcastReceiver, Source},
    config::GroupConfig,
    core,
};
use color_eyre::Result;
use serenity::{
    all::{CreateAttachment, ExecuteWebhook, Http, Webhook},
    async_trait,
};

use super::DiscordBridge;

#[async_trait]
impl BroadcastReceiver for DiscordBridge {
    async fn receive(&self, group: &GroupConfig, message: &core::Message) -> Result<()> {
        let http = Http::new("");
        let webhook = Webhook::from_url(&http, &group.discord_webhook).await?;

        let builder = ExecuteWebhook::new()
            .content(&message.content)
            .username(message.author.full_name());

        let builder = match &self.storage {
            Some(storage) => match &message.author.avatar {
                Some(avatar) => {
                    builder.avatar_url(storage.lock().await.get_url(avatar).await?)
                }
                None => builder,
            },
            None => builder,
        };

        let mut attachments: Vec<CreateAttachment> = vec![];
        for file in &message.attachments {
            let attachment_old = CreateAttachment::path(&file.file.file_path()).await?;
            let attachment = if file.spoilered {
                CreateAttachment::bytes(
                    attachment_old.data,
                    format!(
                        "SPOILER_{}",
                        &file
                            .file
                            .file_path()
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    )
                    .to_owned(),
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

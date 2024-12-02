use crate::{
    broadcast::{BroadcastReceiver, MessageEvent, Source},
    config::GroupConfig,
};
use color_eyre::{eyre::eyre, Result};
use serenity::{
    all::{CreateAttachment, EditWebhookMessage, ExecuteWebhook, Http, Webhook},
    async_trait,
};
use tracing::*;

use super::DiscordBridge;

#[async_trait]
impl BroadcastReceiver for DiscordBridge {
    #[instrument(skip_all)]
    async fn receive(&self, group: &GroupConfig, event: &MessageEvent) -> Result<()> {
        let http = Http::new("");
        let webhook = Webhook::from_url(&http, &group.discord_webhook).await?;

        match event {
            MessageEvent::Create(core_msg) => {
                let builder = ExecuteWebhook::new()
                    .content(&core_msg.content)
                    .username(core_msg.author.full_name());

                let builder = match &self.storage {
                    Some(storage) => match &core_msg.author.avatar {
                        Some(avatar) => {
                            builder.avatar_url(storage.lock().await.get_url(avatar).await?)
                        }
                        None => builder,
                    },
                    None => builder,
                };

                let mut attachments: Vec<CreateAttachment> = vec![];
                for file in &core_msg.attachments {
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
                let msg = webhook.execute(&http, true, builder).await?;

                if let Some(msg) = msg {
                    let mut cache = self.cache.lock().await;
                    cache.dsc_core_cache.insert(msg.id, core_msg.id);
                    cache
                        .core_dsc_cache
                        .insert(core_msg.id, (msg.id, String::new()));
                };
            }

            MessageEvent::Update(core_id, text) => {
                // get dsc message id
                let (dsc_id, header) = match self.cache.lock().await.core_dsc_cache.get(core_id) {
                    Some((id, header)) => (*id, header.clone()),
                    None => {
                        return Err(eyre!(
                            "could not find core message {core_id} in Discord cache"
                        ))
                    }
                };

                let builder = EditWebhookMessage::new().content(header + text);

                webhook.edit_message(&http, dsc_id, builder).await?;
            }

            _ => todo!(),
        };

        Ok(())
    }

    fn get_receiver_source(&self) -> Source {
        Source::Discord
    }
}

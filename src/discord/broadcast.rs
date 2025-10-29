use crate::{
    broadcast::{BroadcastReceiver, MessageEvent, Source},
    config::GroupConfig,
    core::Author,
};
use color_eyre::{eyre::eyre, Result};
use log::*;
use serenity::{
    all::{CreateAllowedMentions, CreateAttachment, EditWebhookMessage, ExecuteWebhook, Webhook},
    async_trait,
};

use super::DiscordBridge;

#[async_trait]
impl BroadcastReceiver for DiscordBridge {
    async fn receive(&self, group: &GroupConfig, event: &MessageEvent) -> Result<()> {
        let dsc = match &group.discord {
            Some(dsc) => dsc,
            None => return Ok(()),
        };

        let webhook = Webhook::from_url(self.http.clone(), &dsc.webhook).await?;

        match event {
            MessageEvent::Create(core_msg) => {
                // get core ID of reply if possible
                let dsc_reply = match core_msg.in_reply_to {
                    Some(id) => self
                        .cache
                        .lock()
                        .await
                        .core_dsc_cache
                        .get(&id)
                        .map(|result| result.0),
                    None => None,
                };

                // get the message to get the author
                let reply_msg = match dsc_reply {
                    Some(id) => Some(
                        self.http
                            .clone()
                            .get_message(dsc.channel.into(), id)
                            .await?,
                    ),
                    None => None,
                };

                // construct either a mention or a plain string
                let mention = match &core_msg.reply_author {
                    Some(author) => match author.source {
                        Source::Discord => reply_msg
                            .as_ref()
                            .map(|msg| format!("<@{}>", msg.author.id.get())),
                        _ => {
                            let full_author: Author = author.clone().into();
                            Some(format!("**{}**", full_author.full_name(Some(0))))
                        }
                    },
                    None => None,
                };

                let header = match reply_msg {
                    Some(msg) => {
                        format!(
                            "*In reply to {} (https://discord.com/channels/{}/{}/{})*\n",
                            mention.unwrap_or("???".to_owned()),
                            msg.guild_id.unwrap_or_default(),
                            dsc.channel,
                            msg.id.get(),
                        )
                    }
                    None => String::new(),
                };

                let builder = ExecuteWebhook::new()
                    .content(header.clone() + &core_msg.content)
                    .username(core_msg.author.full_name(None))
                    .allowed_mentions(CreateAllowedMentions::new().all_users(true));

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
                let msg = webhook.execute(self.http.clone(), true, builder).await?;

                if let Some(msg) = msg {
                    let mut cache = self.cache.lock().await;
                    cache
                        .dsc_core_cache
                        .insert(msg.id, (core_msg.id, (&core_msg.author).into()));
                    cache.core_dsc_cache.insert(core_msg.id, (msg.id, header));
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

                webhook
                    .edit_message(self.http.clone(), dsc_id, builder)
                    .await?;
            }

            _ => todo!(),
        };

        Ok(())
    }

    fn get_receiver_source(&self) -> Source {
        Source::Discord
    }
}

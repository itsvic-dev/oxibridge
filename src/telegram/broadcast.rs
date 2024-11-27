use crate::{
    broadcast::{BroadcastReceiver, MessageEvent, Source},
    config::GroupConfig,
};
use color_eyre::eyre::{eyre, Result};
use serenity::async_trait;
use teloxide::{
    payloads::{EditMessageTextSetters, SendMessageSetters},
    prelude::Requester,
    types::{ChatId, Recipient},
};
use tracing::*;

use super::{entities::to_string_with_entities, TelegramBridge};

#[async_trait]
impl BroadcastReceiver for TelegramBridge {
    #[instrument(skip_all)]
    async fn receive(&self, group: &GroupConfig, event: &MessageEvent) -> Result<()> {
        match event {
            MessageEvent::Create(core_msg) => {
                let text = format!(
                    "**{}**\n{}",
                    &core_msg.author.full_name(),
                    &core_msg.content
                );

                let parsed = to_string_with_entities(&text);

                let msg = if !core_msg.attachments.is_empty() {
                    todo!();
                } else {
                    self.bot
                        .send_message(
                            Recipient::Id(ChatId(group.telegram_chat)),
                            String::from_utf16_lossy(&parsed.0),
                        )
                        .entities(parsed.1)
                        .await?
                };

                let mut cache = self.cache.lock().await;
                cache
                    .core_tg_cache
                    .insert(core_msg.id, (msg.id, core_msg.author.full_name().clone()));
                cache.tg_core_cache.insert(msg.id, core_msg.id);
                debug!(?cache, "current message cache state");
            }

            MessageEvent::Update(id, content) => {
                // get telegram ID and author name
                let (tg_id, author) = match self.cache.lock().await.core_tg_cache.get(id) {
                    Some((id, str)) => (*id, str.clone()),
                    None => return Err(eyre!("could not find core message {id} on Telegram")),
                };

                let text = format!("**{}**\n{}", &author, &content);

                let parsed = to_string_with_entities(&text);

                self.bot
                    .edit_message_text(
                        ChatId(group.telegram_chat),
                        tg_id,
                        String::from_utf16_lossy(&parsed.0),
                    )
                    .entities(parsed.1)
                    .await?;
            }

            _ => todo!(),
        };

        Ok(())
    }

    fn get_receiver_source(&self) -> Source {
        Source::Telegram
    }
}

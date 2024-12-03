use crate::{
    broadcast::{BroadcastReceiver, MessageEvent, Source},
    config::GroupConfig,
};
use color_eyre::eyre::{eyre, Result};
use serenity::async_trait;
use teloxide::{
    payloads::{EditMessageTextSetters, SendMediaGroupSetters, SendMessageSetters},
    prelude::Requester,
    types::{ChatId, InputFile, InputMedia, InputMediaDocument, Recipient, ReplyParameters},
};
use tracing::*;

use super::{entities::to_string_with_entities, TelegramBridge};

#[async_trait]
impl BroadcastReceiver for TelegramBridge {
    #[instrument(skip_all)]
    async fn receive(&self, group: &GroupConfig, event: &MessageEvent) -> Result<()> {
        let chat_id = Recipient::Id(ChatId(group.telegram_chat));

        match event {
            MessageEvent::Create(core_msg) => {
                let text = format!(
                    "**{}**\n{}",
                    &core_msg.author.full_name(),
                    &core_msg.content
                );

                let parsed = to_string_with_entities(&text);
                let content = String::from_utf16_lossy(&parsed.0);

                // get core ID of reply if possible
                let tg_reply = match core_msg.in_reply_to {
                    Some(id) => self
                        .cache
                        .lock()
                        .await
                        .core_tg_cache
                        .get(&id)
                        .map(|result| result.0),
                    None => None,
                };

                let messages = if !core_msg.attachments.is_empty() {
                    // just send as documents for now
                    let media = core_msg
                        .attachments
                        .iter()
                        .map(|x| {
                            InputMedia::Document(
                                InputMediaDocument::new(
                                    InputFile::file(x.file.file_path())
                                        .file_name(x.filename.clone()),
                                )
                                .caption(&content)
                                .caption_entities(parsed.1.clone()),
                            )
                        })
                        .collect::<Vec<InputMedia>>();
                    self.bot
                        .send_media_group(chat_id, media)
                        .reply_parameters(match tg_reply {
                            Some(id) => ReplyParameters::new(id),
                            None => ReplyParameters::default(),
                        })
                        .await?
                } else {
                    vec![
                        self.bot
                            .send_message(chat_id, content)
                            .entities(parsed.1)
                            .reply_parameters(match tg_reply {
                                Some(id) => ReplyParameters::new(id),
                                None => ReplyParameters::default(),
                            })
                            .await?,
                    ]
                };

                if let Some(msg) = messages.first() {
                    let mut cache = self.cache.lock().await;
                    cache
                        .core_tg_cache
                        .insert(core_msg.id, (msg.id, core_msg.author.full_name().clone()));
                    cache.tg_core_cache.insert(msg.id, core_msg.id);
                };
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

            // this might not work correctly with multi-attachment messages
            MessageEvent::Delete(id) => {
                let tg_id = match self.cache.lock().await.core_tg_cache.get(id) {
                    Some((id, _str)) => *id,
                    None => return Err(eyre!("could not find core message {id} on Telegram")),
                };

                self.bot.delete_message(chat_id, tg_id).await?;
            }
        };

        Ok(())
    }

    fn get_receiver_source(&self) -> Source {
        Source::Telegram
    }
}

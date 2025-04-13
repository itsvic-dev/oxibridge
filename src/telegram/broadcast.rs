use std::{ffi::OsStr, path::Path};

use crate::{
    broadcast::{BroadcastReceiver, MessageEvent, Source},
    config::GroupConfig,
};
use color_eyre::eyre::{eyre, Result};
use serenity::async_trait;
use teloxide::{
    payloads::{EditMessageTextSetters, SendMediaGroupSetters, SendMessageSetters},
    prelude::Requester,
    types::{
        ChatId, InputFile, InputMedia, InputMediaAudio, InputMediaDocument, InputMediaPhoto,
        InputMediaVideo, Recipient, ReplyParameters,
    },
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
                    &core_msg.author.full_name(Some(0)),
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
                    let media = core_msg
                        .attachments
                        .iter()
                        .enumerate()
                        .map(|(i, attachment)| {
                            let file = InputFile::file(attachment.file.file_path())
                                .file_name(attachment.filename.clone());

                            match Path::new(&attachment.filename)
                                .extension()
                                .and_then(OsStr::to_str)
                            {
                                Some("png") | Some("jpg") | Some("jpeg") | Some("webp") => {
                                    let media = InputMediaPhoto::new(file);
                                    let media = match i {
                                        0 => media
                                            .caption(&content)
                                            .caption_entities(parsed.1.clone()),
                                        _ => media,
                                    };
                                    InputMedia::Photo(media)
                                }
                                Some("mp4") | Some("mov") | Some("mkv") | Some("webm") => {
                                    let media = InputMediaVideo::new(file);
                                    let media = match i {
                                        0 => media
                                            .caption(&content)
                                            .caption_entities(parsed.1.clone()),
                                        _ => media,
                                    };
                                    InputMedia::Video(media)
                                }
                                Some("mp3") | Some("wav") | Some("ogg") | Some("flac") => {
                                    let media = InputMediaAudio::new(file);
                                    let media = match i {
                                        0 => media
                                            .caption(&content)
                                            .caption_entities(parsed.1.clone()),
                                        _ => media,
                                    };
                                    InputMedia::Audio(media)
                                }
                                _ => {
                                    let media = InputMediaDocument::new(file);
                                    let media = match i {
                                        0 => media
                                            .caption(&content)
                                            .caption_entities(parsed.1.clone()),
                                        _ => media,
                                    };
                                    InputMedia::Document(media)
                                }
                            }
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
                        .insert(core_msg.id, (msg.id, core_msg.author.full_name(Some(0)).clone()));
                    cache.tg_core_cache.insert(msg.id, (core_msg.id, core_msg.author.clone()));
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

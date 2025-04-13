use std::{path::Path, sync::Arc};

use crate::{broadcast::Source, core::{self, Author}};
use async_tempfile::TempFile;
use teloxide::{
    net::Download,
    prelude::*,
    types::{self, Dice, MediaKind, MessageKind, MessageOrigin, PhotoSize},
};
use tracing::*;

use super::unparse::unparse_entities;

pub fn serialize_die_value(die: Dice) -> String {
    match die.emoji {
        types::DiceEmoji::Dice => match die.value {
            1 => "⚀".to_string(),
            2 => "⚁".to_string(),
            3 => "⚂".to_string(),
            4 => "⚃".to_string(),
            5 => "⚄".to_string(),
            6 => "⚅".to_string(),
            _ => "🎲".to_string(),
        },
        types::DiceEmoji::Darts => {
            tracing::debug!("Darts dice value: {}", die.value);

            // 1 is miss
            // 2 is outer double
            // 3 is outer single
            // 4 is triple ring
            // 5 is inner single
            // 6 is bullseye

            //  a value of 6 currently represents a bullseye, while a value of 1 indicates that the dartboard was missed.
            match die.value {
                1 => "🎯❌ Miss!",
                2 => "🎯🎯 Outer double!",
                3 => "🎯🎯 Outer single!",
                4 => "🎯🎯🎯 Triple ring!",
                5 => "🎯🎯 Inner single!",
                6 => "🎯🎯🎯 Bullseye!",
                _ => "🎯",
            }
            .to_string()
        }
        types::DiceEmoji::Bowling => {
            tracing::debug!("Bowling dice value: {}", die.value);

            // 1 is right gutter
            // 2 is left taking down only 1 pin
            // 3 is mid-left, taking down 3 pins
            // 4 is mid-right, taking down 4 pins
            // 5 is spare (taking down 5 pins)
            // 6 is strike
            // let's do the math

            match die.value {
                1 => "🎳❌ Gutter!",
                2 => "🎳🫡 1 pin down",
                3 => "🎳🫡 Split! 3 pins down",
                4 => "🎳🫡 4 pins down",
                5 => "🎳🫡 Spare! 5 pins down",
                6 => "🎳🎳 Strike!",
                _ => "🎳",
            }
            .to_string()
        }
        types::DiceEmoji::Basketball => {
            // should be similar to football

            tracing::debug!("Basketball dice value: {}", die.value);
            let dunk = die.value >= 4;
            if dunk {
                "🏀🔥".to_string()
            } else {
                "🏀❌".to_string()
            }
        }
        types::DiceEmoji::Football => {
            tracing::debug!("Football dice value: {}", die.value);
            // let's document the mapping here..

            // If emoji is “⚽”, a value of 4 to 5 currently scores a goal, while a value of 1 to 3
            // indicates that the goal was missed. However, this behaviour is undocumented and might be changed by Telegram.

            // 1 is overshot (miss)
            // 2 is top left miss
            // 3 is middle goal?
            // 4 is left goal, hitting right post and going in net
            // 5 is top right goal

            let goal = die.value >= 3;
            if goal {
                "⚽️🥅".to_string()
            } else {
                "⚽️❌".to_string()
            }
        }
        types::DiceEmoji::SlotMachine => {
            let values: [u8; 3] = [
                (die.value - 1) & 3,
                ((die.value - 1) >> 2) & 3,
                ((die.value - 1) >> 4) & 3,
            ];

            // map those values to the correct slot emoji

            fn slot(value: u8) -> String {
                match value {
                    // 0 is BAR
                    0 => "⬛",
                    1 => "🍇",
                    2 => "🍋",
                    3 => "7️⃣",
                    _ => "🎰",
                }
                .to_string()
            }

            let value = format!(
                "[{} {} {}]",
                slot(values[0]),
                slot(values[1]),
                slot(values[2])
            );

            if values.iter().all(|&v| v == 3) {
                format!("{value} 🎉🎉🎉 JACKPOT!!! 🎉🎉🎉", value = value)
            } else {
                value
            }
        }
    }
}

#[instrument(skip(bot, m))]
pub async fn to_core_message(
    bot: Bot,
    m: &Message,
    in_reply_to: Option<u64>,
    reply_author: Option<Arc<Author>>,
) -> color_eyre::Result<core::Message> {
    let tg_author = match m.from.as_ref() {
        Some(author) => author,
        None => return Err(color_eyre::eyre::eyre!("Message has no author")),
    };
    let core_author = to_core_author(bot.clone(), tg_author).await?;

    let (content, attachments) = match &m.kind {
        MessageKind::Common(common) => match &common.media_kind {
            // MediaKind::Text(text) => (text.text.to_owned(), vec![]),
            MediaKind::Text(text) => (unparse_entities(&text.text, text.entities.clone()), vec![]),

            MediaKind::Photo(photo) => {
                let attachment = photo_to_core_file(bot.clone(), &photo.photo).await?;
                (
                    unparse_entities(
                        &photo.caption.clone().unwrap_or("".to_string()), photo.caption_entities.clone()
                    ),
                    vec![core::Attachment {
                        file: attachment,
                        spoilered: photo.has_media_spoiler,
                        filename: String::new(),
                    }],
                )
            }

            MediaKind::Video(video) => {
                let file = bot.get_file(&video.video.file.id).await?;
                let attachment = to_core_file(bot.clone(), &file).await?;
                (
                    unparse_entities(
                        &video.caption.clone().unwrap_or("".to_string()), video.caption_entities.clone()
                    ),
                    vec![core::Attachment {
                        file: attachment,
                        spoilered: video.has_media_spoiler,
                        filename: String::new(),
                    }],
                )
            }

            MediaKind::Animation(animation) => {
                let file = bot.get_file(&animation.animation.file.id).await?;
                let attachment = to_core_file(bot.clone(), &file).await?;
                (
                    unparse_entities(
                        &animation.caption.clone().unwrap_or("".to_string()), animation.caption_entities.clone()
                    ),
                    vec![core::Attachment {
                        file: attachment,
                        spoilered: animation.has_media_spoiler,
                        filename: String::new(),
                    }],
                )
            }

            // plain files
            MediaKind::Document(document) => {
                let file = bot.get_file(&document.document.file.id).await?;
                let attachment = to_core_file(bot.clone(), &file).await?;
                (
                    unparse_entities(
                        &document.caption.clone().unwrap_or("".to_string()), document.caption_entities.clone()
                    ),
                    vec![core::Attachment {
                        file: attachment,
                        spoilered: false,
                        filename: String::new(),
                    }],
                )
            }

            MediaKind::Sticker(sticker) => {
                let sticker = &sticker.sticker;
                match sticker.flags.is_animated {
                    false => {
                        // raster or video sticker
                        let file = bot.get_file(&sticker.file.id).await?;
                        let attachment = to_core_file(bot.clone(), &file).await?;
                        let set_name = match sticker.set_name.clone() {
                            Some(slug) => {
                                let set = bot.get_sticker_set(&slug).await?;
                                // technically discord sugar, but its fine for now
                                format!("[{}](<https://t.me/addstickers/{}>)", &set.title, &slug)
                            }
                            None => "Unknown set".to_owned(),
                        };
                        (
                            format!(
                                "_{} sticker from {}_",
                                &sticker.emoji.clone().unwrap_or("?".to_owned()),
                                &set_name,
                            ),
                            vec![core::Attachment {
                                file: attachment,
                                spoilered: false,
                                filename: String::new(),
                            }],
                        )
                    }
                    true => ("[Animated sticker]".to_owned(), vec![]),
                }
            }

            _ => ("[Unknown media kind]".to_owned(), {
                tracing::warn!("Unknown media kind: {:?}", &common.media_kind);
                vec![]
            }),
        },

        MessageKind::Dice(die) => (
            {
                // dice.emoji implements serde::Serialize, so we can just use it
                let value = serialize_die_value(die.dice.clone());

                format!("_{} rolled a die!_\n{}", core_author.full_name(Some(0)), value)
            },
            vec![],
        ),

        MessageKind::NewChatMembers(members) => (
            {
                let m = members
                    .new_chat_members
                    .iter()
                    .map(|member| member.full_name())
                    .collect::<Vec<String>>()
                    .join(", ");
                format!("_{} joined the chat_", m)
            },
            vec![],
        ),

        MessageKind::LeftChatMember(member) => (
            format!("_{} left the chat_", member.left_chat_member.full_name()),
            vec![],
        ),

        MessageKind::Pinned(pinned) => (
            format!(
                "_{} pinned a message:_\n{}",
                core_author.full_name(Some(0)),
                pinned
                    .pinned
                    .as_ref()
                    .regular_message()
                    .map_or("", |m| m.text().unwrap_or(""))
            ),
            vec![],
        ),

        MessageKind::Empty {} => ("[Empty message]".to_owned(), vec![]),

        _ => ("[Unknown message kind]".to_owned(), {
            tracing::warn!("Unknown message kind: {:?}", &m.kind);
            vec![]
        }),
    };

    let forwarded_header = match m.forward_origin() {
        Some(MessageOrigin::User {
            date: _,
            sender_user,
        }) => {
            format!("*Forwarded from {}*", &sender_user.full_name())
        }

        Some(MessageOrigin::HiddenUser {
            date: _,
            sender_user_name,
        }) => format!("*Forwarded from {}*", &sender_user_name),

        Some(MessageOrigin::Chat {
            date: _,
            sender_chat: chat,
            author_signature,
        }) => match author_signature {
            Some(signature) => format!(
                "*Forwarded from {} ({})*",
                chat.title().unwrap_or("an unknown chat"),
                &signature
            ),
            None => format!(
                "*Forwarded from {}*",
                chat.title().unwrap_or("an unknown chat")
            ),
        },

        Some(MessageOrigin::Channel {
            date: _,
            chat,
            message_id: _,
            author_signature,
        }) => match author_signature {
            Some(signature) => format!(
                "*Forwarded from {} ({})*",
                chat.title().unwrap_or("an unknown channel"),
                &signature
            ),
            None => format!(
                "*Forwarded from {}*",
                chat.title().unwrap_or("an unknown channel")
            ),
        },

        _ => "".to_owned(),
    };

    let content = [forwarded_header, content].join("\n").trim().to_owned();

    Ok(core::Message::new(Arc::new(core_author), content, attachments, in_reply_to, reply_author).await)
}

#[instrument(skip(bot))]
async fn to_core_author(bot: Bot, author: &types::User) -> color_eyre::Result<core::Author> {
    let photos = bot.get_user_profile_photos(author.id).await?;
    let photo = photos.photos.first();

    let core_file = match photo {
        Some(photo) => match photo_to_core_file(bot, photo).await {
            Ok(file) => Some(file),
            Err(_) => None,
        },
        None => None,
    };

    Ok(core::Author {
        display_name: Some(author.full_name()),
        username: match &author.username {
            Some(username) => username.clone(),
            None => format!("id{}", author.id.0),
        },
        avatar: core_file,
        source: Source::Telegram,
    })
}

pub async fn photo_to_core_file(bot: Bot, photo: &[PhotoSize]) -> color_eyre::Result<TempFile> {
    if let Some(photo) = photo.last() {
        let file = bot.get_file(&photo.file.id).await?;
        to_core_file(bot, &file).await
    } else {
        Err(color_eyre::eyre::eyre!("No photo found"))
    }
}

#[instrument(skip(bot))]
pub async fn to_core_file(bot: Bot, file: &teloxide::types::File) -> color_eyre::Result<TempFile> {
    let extension = Path::new(&file.path)
        .extension()
        .and_then(|ext| ext.to_str())
        .ok_or_else(|| color_eyre::eyre::eyre!("Failed to get file extension"))?;
    let mut tmpfile =
        async_tempfile::TempFile::new_with_name(&format!("{}.{}", &file.unique_id, extension))
            .await?;
    // let path = core::get_tmp_dir()?.join(format!("{}.{}", &file.unique_id, extension));
    // let mut dst = fs::File::create(&path.file_path()).await?;
    bot.download_file(&file.path, &mut tmpfile).await?;

    // Ok(core::File {
    //     name: file.path.replace('/', "_"),
    //     path: path.into_path(),
    // })
    Ok(tmpfile)
}

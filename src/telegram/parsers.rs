use std::path::Path;

use crate::core;
use async_tempfile::TempFile;
use teloxide::{
    net::Download,
    prelude::*,
    types::{self, MediaKind, MessageKind, MessageOrigin, PhotoSize},
};
use tracing::*;

#[instrument(skip(bot, m))]
pub async fn to_core_message(bot: Bot, m: &Message) -> color_eyre::Result<core::Message> {
    let tg_author = match m.from.as_ref() {
        Some(author) => author,
        None => return Err(color_eyre::eyre::eyre!("Message has no author")),
    };
    let core_author = to_core_author(bot.clone(), tg_author).await?;

    let (content, attachments) = match &m.kind {
        MessageKind::Common(common) => match &common.media_kind {
            MediaKind::Text(text) => (text.text.to_owned(), vec![]),

            MediaKind::Photo(photo) => {
                let attachment = photo_to_core_file(bot.clone(), &photo.photo).await?;
                (
                    photo.caption.clone().unwrap_or("".to_owned()),
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
                    video.caption.clone().unwrap_or("".to_owned()),
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
                    animation.caption.clone().unwrap_or("".to_owned()),
                    vec![core::Attachment {
                        file: attachment,
                        spoilered: animation.has_media_spoiler,
                        filename: String::new(),
                    }],
                )
            }

            MediaKind::Sticker(sticker) => {
                let sticker = &sticker.sticker;
                match (sticker.flags.is_animated, sticker.flags.is_video) {
                    (false, false) => {
                        // raster sticker
                        let file = bot.get_file(&sticker.file.id).await?;
                        let attachment = to_core_file(bot.clone(), &file).await?;
                        let set_name = match sticker.set_name.clone() {
                            Some(slug) => {
                                let set = bot.get_sticker_set(&slug).await?;
                                // technically discord sugar, but its fine
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
                    _ => ("[Animated or video sticker]".to_owned(), vec![]),
                }
            }

            _ => ("[Unknown media kind]".to_owned(), vec![]),
        },
        _ => ("[Unknown message kind]".to_owned(), vec![]),
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

    Ok(core::Message::new(core_author, content, attachments).await)
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
        username: author.username.clone().unwrap_or("Unknown".to_string()),
        avatar: core_file,
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

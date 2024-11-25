use crate::core;
use teloxide::{net::Download, prelude::*};
use tokio::fs;
use tracing::*;

#[instrument(skip(bot, m))]
pub async fn to_core_message(bot: Bot, m: Message) -> color_eyre::Result<core::Message> {
    let tg_author = m.from.as_ref().unwrap();

    let photos = bot.get_user_profile_photos(tg_author.id).await?;
    let photo = photos.photos.get(0);

    let core_file = match photo {
        Some(photo) => {
            let photo = photo.get(0).unwrap();
            let file = bot.get_file(&photo.file.id).await?;
            match to_core_file(bot, &file).await {
                Ok(file) => Some(file),
                Err(_) => None,
            }
        }
        None => None,
    };

    let core_author = core::Author {
        display_name: tg_author.full_name(),
        username: tg_author.username.clone().unwrap_or("Unknown".to_string()),
        avatar: core_file,
    };

    Ok(core::Message {
        author: core_author,
        content: m.text().unwrap().to_string(),
        attachments: [].to_vec(),
    })
}

#[instrument]
pub async fn to_core_file(
    bot: Bot,
    file: &teloxide::types::File,
) -> color_eyre::Result<core::File> {
    let path = core::get_tmp_dir()?.join(&file.unique_id);
    let mut dst = fs::File::create(&path).await?;
    bot.download_file(&file.path, &mut dst).await?;

    Ok(core::File {
        name: file.path.to_string(),
        path: path,
    })
}

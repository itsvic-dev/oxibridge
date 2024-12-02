use crate::core;
use async_tempfile::TempFile;
use color_eyre::eyre::Result;
use serenity::{
    all::{Attachment, Message, User},
    futures::StreamExt,
};
use tokio::io::AsyncWriteExt;

pub async fn to_core_message(message: &Message, in_reply_to: Option<u64>) -> Result<core::Message> {
    let dsc_author = &message.author;
    let core_author = to_core_author(dsc_author)?;

    let mut attachments: Vec<core::Attachment> = Vec::new();

    for attachment in &message.attachments {
        attachments.push(to_core_attachment(attachment).await?);
    }

    Ok(core::Message::new(
        core_author,
        message.content.clone(),
        attachments,
        in_reply_to,
    )
    .await)
}

pub fn to_core_author(author: &User) -> Result<core::Author> {
    Ok(core::Author {
        username: author.name.clone(),
        display_name: author.global_name.clone(),
        avatar: None, // no need to care rn, tg doesn't need it
    })
}

pub async fn to_core_attachment(attachment: &Attachment) -> Result<core::Attachment> {
    let mut stream = reqwest::get(&attachment.url).await?.bytes_stream();
    let mut file = TempFile::new().await?;

    while let Some(item) = stream.next().await {
        file.write_all(&item?).await?;
    }

    file.flush().await?;

    Ok(core::Attachment {
        file,
        spoilered: attachment.filename.starts_with("SPOILER_"),
        filename: attachment.filename.clone(),
    })
}

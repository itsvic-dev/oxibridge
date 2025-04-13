use std::sync::{Arc, LazyLock};

use crate::core::{self, Author};
use async_tempfile::TempFile;
use color_eyre::eyre::Result;
use regex::Regex;
use serenity::{
    all::{Attachment, Http, Message, User},
    futures::StreamExt,
};
use tokio::io::AsyncWriteExt;

static MENTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<@([0-9]+)>").unwrap());

pub async fn to_core_message(
    message: &Message,
    in_reply_to: Option<u64>,
    reply_author: Option<Arc<Author>>,
    http: &Http,
) -> Result<core::Message> {
    let dsc_author = &message.author;
    let core_author = Arc::new(to_core_author(dsc_author)?);

    let mut attachments: Vec<core::Attachment> = Vec::new();

    for attachment in &message.attachments {
        attachments.push(to_core_attachment(attachment).await?);
    }

    let mut content = message.content.clone();

    // find and turn mentions into "@dc/username" format
    for (_, [id_str]) in MENTION_RE
        .captures_iter(&message.content)
        .map(|c| c.extract())
    {
        let id = id_str.parse::<u64>()?;
        let user = http.get_user(id.into()).await?;
        content = content.replace(&format!("<@{id}>"), &format!("@dc/{}", user.name));
    }

    Ok(core::Message::new(core_author, content, attachments, in_reply_to, reply_author).await)
}

pub fn to_core_author(author: &User) -> Result<core::Author> {
    Ok(core::Author {
        username: format!("dc/{}", &author.name),
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

use std::sync::LazyLock;

use crate::{broadcast::Source, core::{self, PartialAuthor}, discord::refresh::refresh_cdn_links};
use async_tempfile::TempFile;
use color_eyre::eyre::Result;
use regex::Regex;
use serenity::{
    all::{Attachment, Http, Message, User},
    futures::StreamExt,
};
use tokio::io::AsyncWriteExt;

static MENTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<@([0-9]+)>").unwrap());
static CDN_LINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https://(?:cdn\.discordapp\.com|media\.discordapp\.net)/attachments/\d+/\d+/[a-zA-Z0-9.%_\-]+(?:\?[\w\d=&]+)?").unwrap());

pub async fn to_core_message(
    message: &Message,
    in_reply_to: Option<u64>,
    reply_author: Option<PartialAuthor>,
    http: &Http,
) -> Result<core::Message> {
    let dsc_author = &message.author;
    let core_author = to_core_author(dsc_author)?;

    let mut attachments: Vec<core::Attachment> = Vec::new();

    for attachment in &message.attachments {
        attachments.push(to_core_attachment(attachment).await?);
    }

    let content = parse_content(&message.content, http).await?;
    Ok(core::Message::new(core_author, content, attachments, in_reply_to, reply_author).await)
}

pub fn to_core_author(author: &User) -> Result<core::Author> {
    Ok(core::Author {
        username: author.name.to_owned(),
        display_name: author.global_name.clone(),
        avatar: None, // no need to care rn, tg doesn't need it
        source: Source::Discord,
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

pub async fn parse_content(content: &str, http: &Http) -> Result<String> {
    let mut new_content = content.to_string();

    // find and turn mentions into "@dc/username" format
    for (_, [id_str]) in MENTION_RE
        .captures_iter(content)
        .map(|c| c.extract())
    {
        let id = id_str.parse::<u64>()?;
        let user = http.get_user(id.into()).await;
        if let Ok(user) = user {
            new_content = new_content.replace(&format!("<@{id}>"), &format!("@dc/{}", user.name));
        }
    }

    get_content_with_refreshed_links(http, &new_content).await
}

async fn get_content_with_refreshed_links(http: &Http, content: &str) -> Result<String> {
    let links: Vec<&str> = CDN_LINK_RE.find_iter(content).map(|x| x.as_str()).collect();

    let mut content = content.to_string();

    if !links.is_empty() {
        refresh_cdn_links(http, &links).await?.iter().for_each(|x| { content = content.replace(&x.original, &x.refreshed); });
    }

    Ok(content)
}

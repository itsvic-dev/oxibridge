use std::env;

use crate::core;
use color_eyre::eyre::{Context, Result};
use serenity::all::{CreateAttachment, ExecuteWebhook, Http, Webhook};

pub async fn broadcast_message(message: &core::Message) -> Result<()> {
    let http = Http::new("");
    let webhook_url =
        env::var("DISCORD_WEBHOOK").context("Set the DISCORD_WEBHOOK environment variable.")?;
    let webhook = Webhook::from_url(&http, &webhook_url).await?;

    let builder = ExecuteWebhook::new()
        .content(&message.content)
        .username(format!(
            "{} ({})",
            &message.author.display_name, &message.author.username
        ));

    let mut attachments: Vec<CreateAttachment> = vec![];
    for file in &message.attachments {
        let attachment_old = CreateAttachment::path(&file.file.path).await?;
        let attachment = if file.spoilered {
            CreateAttachment::bytes(
                attachment_old.data,
                format!("SPOILER_{}", &file.file.name).to_owned(),
            )
        } else {
            attachment_old
        };
        attachments.push(attachment);
    }

    let builder = builder.add_files(attachments);

    webhook.execute(&http, false, builder).await?;

    Ok(())
}

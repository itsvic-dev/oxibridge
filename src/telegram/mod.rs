use teloxide::prelude::*;
use tracing::*;

use crate::core;

#[instrument(skip(m))]
async fn message_handle(m: Message) -> color_eyre::Result<()> {
    debug!(?m, "message received");

    let core_message = to_core_message(m)?;
    debug!(?core_message, "parsed core message");

    Ok(())
}

fn to_core_message(m: Message) -> color_eyre::Result<core::Message> {
    let tg_author = m.from.as_ref().unwrap();

    let core_author = core::Author {
        display_name: tg_author.full_name(),
        username: tg_author.username.clone().unwrap_or("Unknown".to_string()),
        avatar: None,
    };

    Ok(core::Message {
        author: core_author,
        content: m.text().unwrap().to_string(),
        attachments: [].to_vec(),
    })
}

#[instrument]
pub async fn start() {
    info!("setting up telegram bot");
    let bot = Bot::from_env();
    let handler = dptree::entry().branch(Update::filter_message().endpoint(message_handle));
    Dispatcher::builder(bot, handler).build().dispatch().await;
}

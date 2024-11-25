use teloxide::prelude::*;
use tracing::*;

use crate::broadcast::{broadcast, Source};

mod parsers;
use self::parsers::*;

#[instrument(skip(bot, m))]
async fn message_handle(bot: Bot, m: Message) -> color_eyre::Result<()> {
    let core_message = to_core_message(bot, m).await?;
    debug!(?core_message, "parsed core message");

    broadcast(&core_message, Source::Telegram).await?;

    Ok(())
}

#[instrument]
pub async fn start() {
    info!("setting up telegram bot");
    let bot = Bot::from_env();
    let handler = dptree::entry().branch(Update::filter_message().endpoint(message_handle));
    Dispatcher::builder(bot, handler).build().dispatch().await;
}

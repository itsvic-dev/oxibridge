use crate::{core::Message, discord};
use color_eyre::Result;
use tracing::*;

#[derive(Debug, PartialEq)]
pub enum Source {
    Discord,
    Telegram,
}

///
/// Broadcasts a message coming from a source channel to other channels.
///
#[instrument]
pub async fn broadcast(message: &Message, source: Source) -> Result<()> {
    if source != Source::Discord {
        discord::broadcast_message(&message).await?;
    }

    Ok(())
}

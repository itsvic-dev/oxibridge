use crate::{core::Message, discord};
use color_eyre::Result;
use tracing::*;

#[derive(Debug, PartialEq)]
pub enum Source {
    Discord,
    Telegram,
}

/**
Broadcasts a message coming from a source channel to other channels.
*/
#[instrument(skip(message, source))]
pub async fn broadcast(message: &Message, source: Source) -> Result<()> {
    // Err(eyre!("not implemented"))
    debug!("to broadcast: {message:?} from {source:?}");

    if source != Source::Discord {
        discord::broadcast_message(&message).await?;
    }

    Ok(())
}

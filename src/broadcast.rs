use crate::core::Message;
use color_eyre::{eyre::eyre, Result};
use tracing::*;

#[derive(Debug)]
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
    Ok(())
}

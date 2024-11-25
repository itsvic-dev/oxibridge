use crate::core::Message;
use color_eyre::Result;
use tracing::*;

#[derive(Debug)]
pub enum Source {
    Discord,
    Telegram,
}

/**
 * Broadcasts a message coming from a source channel to other channels.
 */
#[instrument]
pub async fn broadcast(message: &Message, source: Source) -> Result<()> {
    todo!()
}

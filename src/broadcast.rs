use std::sync::Arc;

use crate::{config::GroupConfig, core::Message};
use color_eyre::Result;
use serenity::async_trait;
use tracing::instrument;

#[derive(Debug, PartialEq)]
pub enum Source {
    Discord,
    Telegram,
}

#[derive(Debug)]
pub enum MessageEvent {
    Create(Message),
    Update(u64, String),
    Delete(u64),
}

pub struct Broadcaster {
    sources: Vec<Arc<dyn BroadcastReceiver>>,
}

impl Broadcaster {
    pub fn init() -> Self {
        Self { sources: vec![] }
    }

    pub fn add_receiver(&mut self, receiver: Arc<dyn BroadcastReceiver>) -> &mut Self {
        self.sources.push(receiver);
        self
    }

    #[instrument(skip_all)]
    pub async fn broadcast(
        &self,
        group: &GroupConfig,
        event: &MessageEvent,
        source: Source,
    ) -> Result<()> {
        for receiver in &self.sources {
            if receiver.get_receiver_source() != source {
                receiver.receive(group, event).await?;
            }
        }

        Ok(())
    }
}

#[async_trait]
pub trait BroadcastReceiver: Send + Sync {
    async fn receive(&self, group: &GroupConfig, event: &MessageEvent) -> Result<()>;
    fn get_receiver_source(&self) -> Source;
}

use std::sync::Arc;

use crate::core::Message;
use color_eyre::Result;
use serenity::async_trait;

#[derive(Debug, PartialEq)]
pub enum Source {
    Discord,
    Telegram,
}

pub struct Broadcaster {
    sources: Vec<Arc<dyn BroadcastReceiver>>,
}

impl Broadcaster {
    pub fn init() -> Broadcaster {
        Broadcaster { sources: vec![] }
    }

    pub fn add_receiver(&mut self, receiver: Arc<dyn BroadcastReceiver>) {
        self.sources.push(receiver);
    }

    pub async fn broadcast(&self, message: &Message, source: Source) -> Result<()> {
        for receiver in &self.sources {
            if receiver.get_receiver_source() != source {
                receiver.receive(&message).await?;
            }
        }

        Ok(())
    }
}

#[async_trait]
pub trait BroadcastReceiver: Send + Sync {
    async fn receive(&self, message: &Message) -> Result<()>;
    fn get_receiver_source(&self) -> Source;
}

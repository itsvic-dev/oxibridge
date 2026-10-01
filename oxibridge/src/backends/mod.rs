use std::error::Error;

use crate::config::{BackendConfig, BackendKind, GroupBackendConfig};
use log::{debug, warn};
use tokio::sync::broadcast::{self, error::RecvError};

mod file;

pub fn get_backend(
    name: &str,
    backend_config: &BackendConfig,
    group_configs: &[BackendGroup],
) -> Box<dyn self::Backend> {
    debug!("Loading backend '{}' ({:?})", name, backend_config.kind);
    match backend_config.kind {
        BackendKind::File => Box::new(file::FileBackend::new(name, backend_config, group_configs)),
        _ => todo!(),
    }
}

#[async_trait::async_trait]
pub trait Backend {
    fn new(name: &str, config: &BackendConfig, group_configs: &[BackendGroup]) -> Self
    where
        Self: Sized;

    async fn start(&self) -> Result<(), Box<dyn Error>>;
}

/// A backend's view of a group it takes part in.
#[derive(Debug, Clone)]
pub struct BackendGroup {
    pub name: String,
    pub backend_name: String,
    pub config: GroupBackendConfig,
    pub tx: broadcast::Sender<BackendMessage>,
}

impl BackendGroup {
    /// Broadcasts a message to the other backends in this group.
    pub fn send(&self, content: crate::core::Message) {
        let message = BackendMessage {
            group_name: self.name.clone(),
            backend_name: self.backend_name.clone(),
            content,
        };
        if self.tx.send(message).is_err() {
            warn!("group '{}' has no receivers", self.name);
        }
    }

    /// Subscribes to messages sent by the other backends in this group.
    pub fn subscribe(&self) -> GroupReceiver {
        GroupReceiver {
            group_name: self.name.clone(),
            backend_name: self.backend_name.clone(),
            rx: self.tx.subscribe(),
        }
    }
}

pub struct GroupReceiver {
    group_name: String,
    backend_name: String,
    rx: broadcast::Receiver<BackendMessage>,
}

impl GroupReceiver {
    /// Waits for the next message from another backend.
    ///
    /// Returns [`None`] once all senders in the group are gone.
    pub async fn recv(&mut self) -> Option<BackendMessage> {
        loop {
            match self.rx.recv().await {
                Ok(message) if message.backend_name != self.backend_name => return Some(message),
                Ok(_) => {}
                Err(RecvError::Lagged(count)) => warn!(
                    "backend '{}' skipped {count} messages in group '{}'",
                    self.backend_name, self.group_name
                ),
                Err(RecvError::Closed) => return None,
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct BackendMessage {
    pub group_name: String,
    pub backend_name: String,
    pub content: crate::core::Message,
}

use std::{error::Error, sync::Arc};

use crate::{
    config::{BackendConfig, GroupBackendConfig},
    database::Database,
    storage::R2Storage,
};
use log::{debug, warn};
use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        watch,
    },
    task::JoinSet,
};

pub mod discord;
pub mod file;
pub mod irc;
pub mod telegram;

/// Creates the backend described by `backend_config`.
///
/// # Errors
/// Returns an error if the backend rejects its configuration.
pub fn get_backend(
    name: &str,
    backend_config: &BackendConfig,
    group_configs: &[BackendGroup],
    database: &Database,
    storage: Option<&Arc<R2Storage>>,
) -> Result<Box<dyn self::Backend>, Box<dyn Error>> {
    debug!("Loading backend '{name}'");
    Ok(match backend_config {
        BackendConfig::File(config) => Box::new(file::FileBackend::new(
            name,
            config,
            group_configs,
            database.clone(),
        )?),
        BackendConfig::Irc(config) => Box::new(irc::IrcBackend::new(
            name,
            config,
            group_configs,
            database.clone(),
        )?),
        BackendConfig::Telegram(config) => Box::new(telegram::TelegramBackend::new(
            name,
            config,
            group_configs,
            database.clone(),
        )?),
        BackendConfig::Discord(config) => Box::new(discord::DiscordBackend::new(
            name,
            config,
            group_configs,
            database.clone(),
            storage.cloned(),
        )?),
    })
}

#[async_trait::async_trait]
pub trait Backend {
    /// Starts the backend. Long-running work is spawned onto `tasks`.
    async fn start(&self, tasks: &mut JoinSet<()>) -> Result<(), Box<dyn Error>>;
}

/// A backend's view of a group it takes part in.
#[derive(Debug, Clone)]
pub struct BackendGroup {
    pub name: String,
    pub backend_name: String,
    pub config: GroupBackendConfig,
    pub tx: broadcast::Sender<BackendMessage>,
    /// Becomes true once all backends have started, and so subscribed to their groups.
    pub ready: watch::Receiver<bool>,
}

impl BackendGroup {
    /// Broadcasts an event to the other backends in this group.
    ///
    /// Waits until all backends have started, so that none of them misses the event.
    pub async fn send(&self, event: MessageEvent) {
        if self.ready.clone().wait_for(|ready| *ready).await.is_err() {
            warn!("group '{}' was never ready", self.name);
            return;
        }
        let message = BackendMessage {
            group_name: self.name.clone(),
            backend_name: self.backend_name.clone(),
            event,
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
    pub event: MessageEvent,
}

#[derive(Clone, Debug)]
pub enum MessageEvent {
    Create(crate::core::Message),
    /// Carries the full new message. Its ID is the ID of the edited message.
    Edit(crate::core::Message),
    /// Carries the full message after its reactions changed, also to the backend it came from.
    Reactions(crate::core::Message),
    /// Carries the ID of the deleted message.
    Delete(i64),
}

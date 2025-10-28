use crate::config::{BackendConfig, GroupBackendConfig};
use tokio::sync::broadcast;
use tracing::debug;

mod file;

pub fn get_backend(
    name: &str,
    backend_config: &BackendConfig,
    group_name: &str,
    group_config: &GroupBackendConfig,
    tx: broadcast::Sender<BackendMessage>,
) -> Option<Box<dyn self::Backend>> {
    debug!(
        "Loading backend '{}' of kind '{}' for group '{}'",
        name, backend_config.kind, group_name
    );
    match backend_config.kind.as_str() {
        "file" => Some(Box::new(file::FileBackend::new(
            name,
            backend_config,
            group_name,
            group_config,
            tx,
        ))),
        _ => None,
    }
}

#[async_trait::async_trait]
pub trait Backend {
    fn new(
        name: &str,
        config: &BackendConfig,
        group_name: &str,
        group_config: &GroupBackendConfig,
        tx: broadcast::Sender<BackendMessage>,
    ) -> Self
    where
        Self: Sized;

    async fn start(&self);
}

#[derive(Clone, Debug)]
pub struct BackendMessage {
    pub group_name: String,
    pub backend_name: String,
    pub content: crate::core::Message,
}

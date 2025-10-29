use crate::config::{BackendConfig, BackendKind, GroupBackendConfig};
use log::debug;
use tokio::sync::broadcast;

mod file;

pub fn get_backend(
    name: &str,
    backend_config: &BackendConfig,
    group_name: &str,
    group_config: &GroupBackendConfig,
    tx: broadcast::Sender<BackendMessage>,
) -> Box<dyn self::Backend> {
    debug!(
        "Loading backend '{}' ({:?}) for group '{}'",
        name, backend_config.kind, group_name
    );
    match backend_config.kind {
        BackendKind::File => Box::new(file::FileBackend::new(
            name,
            backend_config,
            group_name,
            group_config,
            tx,
        )),
        _ => todo!(),
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

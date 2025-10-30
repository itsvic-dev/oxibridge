use std::error::Error;

use crate::config::{BackendConfig, BackendKind, GroupBackendConfig};
use log::debug;
use tokio::sync::broadcast;

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

#[derive(Debug, Clone)]
pub struct BackendGroup {
    pub name: String,
    pub config: GroupBackendConfig,
    pub tx: broadcast::Sender<BackendMessage>,
}

#[derive(Clone, Debug)]
pub struct BackendMessage {
    pub group_name: String,
    pub backend_name: String,
    pub content: crate::core::Message,
}

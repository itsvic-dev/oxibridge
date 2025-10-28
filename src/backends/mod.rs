use tracing::debug;

use crate::config::BackendConfig;

pub fn get_backend(name: &str, config: &BackendConfig) -> Option<Box<dyn self::Backend>> {
    debug!("Loading backend '{}' of kind '{}'", name, config.kind);
    match config.kind.as_str() {
        _ => None,
    }
}

pub trait Backend {
    fn new(config: &BackendConfig) -> Self
    where
        Self: Sized;
}

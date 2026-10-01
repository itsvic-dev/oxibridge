use std::error::Error;

use log::{debug, error, info};
use tokio::task::JoinSet;

mod backends;
mod config;
mod core;
mod storage;
pub use config::Config;

use crate::backends::{BackendGroup, BackendMessage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    setup_logging()?;
    info!("Hello, world!");

    let paths = std::env::var("CONFIG_FILE").unwrap_or_else(|_| "config.yml".to_owned());
    let config = Config::load(&paths.split(':').collect::<Vec<_>>()).await?;
    config.validate()?;

    let groups: Vec<_> = config
        .groups
        .iter()
        .map(|(name, config)| {
            let (tx, _) = tokio::sync::broadcast::channel::<BackendMessage>(32);
            (name, config, tx)
        })
        .collect();

    let backends = config
        .backends
        .iter()
        .map(|(name, backend)| -> Result<_, Box<dyn Error>> {
            let backend_groups: Vec<_> = groups
                .iter()
                .filter(|(_, config, _)| config.contains_key(name))
                .map(|(group_name, config, tx)| BackendGroup {
                    name: (*group_name).clone(),
                    backend_name: name.clone(),
                    config: config[name].clone(),
                    tx: tx.clone(),
                })
                .collect();

            Ok((name, backends::get_backend(name, backend, &backend_groups)?))
        })
        .collect::<Result<Vec<_>, _>>()?;

    // we don't need to keep groups around anymore, drop them so oxibridge can cleanly shut down once all group senders get dropped
    std::mem::drop(groups);

    let mut tasks = JoinSet::new();
    for (name, backend) in backends {
        debug!("Bringing up backend {name}");
        backend.start(&mut tasks).await?;
    }

    while let Some(result) = tasks.join_next().await {
        if let Err(e) = result {
            error!("backend task failed: {e}");
        }
    }

    Ok(())
}

fn setup_logging() -> Result<(), Box<dyn Error>> {
    color_eyre::install()?;
    let mut builder = env_logger::builder();

    builder
        .filter(Some("oxibridge"), log::LevelFilter::Debug)
        .try_init()?;

    Ok(())
}

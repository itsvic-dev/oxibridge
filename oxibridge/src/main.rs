use std::error::Error;

use color_eyre::Section;
use log::*;

mod backends;
mod config;
mod core;
mod storage;
mod tasks;
pub use config::Config;

use crate::backends::{BackendGroup, BackendMessage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    setup_logging()?;
    info!("Hello, world!");

    let config = String::from_utf8(tokio::fs::read(
        std::env::var("CONFIG_FILE").unwrap_or("config.yml".to_owned())
    ).await.suggestion(
        "Create a `config.yml` file and fill it out. Look at `config.example.yml` for reference.",
    )?)?;

    // TODO: validate config
    let config: Config = serde_yaml::from_str(&config)?;

    let groups: Vec<_> = config
        .groups
        .iter()
        .map(|(name, config)| {
            let (tx, _) = tokio::sync::broadcast::channel::<BackendMessage>(32);
            (name, config, tx)
        })
        .collect();

    let backends: Vec<_> = config
        .backends
        .iter()
        .map(|(name, backend)| {
            let backend_groups: Vec<_> = groups
                .iter()
                .filter(|(_, config, _)| config.contains_key(name))
                .map(|(group_name, config, tx)| BackendGroup {
                    name: group_name.to_string(),
                    config: config[name].clone(),
                    tx: tx.clone(),
                })
                .collect();

            (name, backends::get_backend(name, backend, &backend_groups))
        })
        .collect();

    for (name, backend) in backends {
        debug!("Bringing up backend {name}");
        backend.start().await?;
    }

    futures::future::join_all(tasks::get_tasks()).await;

    Ok(())
}

fn setup_logging() -> Result<(), Box<dyn Error>> {
    color_eyre::install()?;
    let mut builder = colog::default_builder();
    builder.filter(Some("oxibridge"), log::LevelFilter::Debug);
    builder.init();

    Ok(())
}

use std::sync::Arc;

use color_eyre::{eyre::Result, Section};
use tracing::*;
use tracing_subscriber::{filter::LevelFilter, layer::SubscriberExt, EnvFilter, Layer};

mod config;
mod core;
mod storage;
pub use config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env()?
        .add_directive("oxibridge=debug".parse()?);

    let subscriber = tracing_subscriber::Registry::default()
        .with(
            tracing_subscriber::fmt::layer()
                .without_time()
                .compact()
                .with_filter(filter),
        )
        .with(tracing_error::ErrorLayer::default());
    tracing::subscriber::set_global_default(subscriber)?;

    info!("hello, world!");

    debug!("reading config file");
    let config = String::from_utf8(tokio::fs::read(
        std::env::var("CONFIG_FILE").unwrap_or("config.yml".to_owned())
    ).await.suggestion(
        "Create a `config.yml` file and fill it out. Look at `config.example.yml` for reference.",
    )?)?;

    // TODO: validate config
    let config: Arc<Config> = Arc::new(serde_yaml::from_str(&config)?);
    debug!("config file read successfully: {config:#?}");

    Ok(())
}

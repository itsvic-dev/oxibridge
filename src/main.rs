use std::sync::Arc;

use broadcast::Broadcaster;
use color_eyre::{eyre::Result, Section};
use tokio::sync::Mutex;
use tracing::*;
use tracing_subscriber::{filter::LevelFilter, layer::SubscriberExt, EnvFilter, Layer};

mod broadcast;
mod config;
mod core;
mod discord;
mod storage;
mod telegram;
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
    let config = String::from_utf8(tokio::fs::read("config.yml").await.suggestion(
        "Create a `config.yml` file and fill it out. Look at `config.example.yml` for reference.",
    )?)?;
    let config: Arc<Config> = Arc::new(serde_yaml::from_str(&config)?);

    let storage = if let Some(config) = &config.shared.r2 {
        Some(Arc::new(Mutex::new(storage::R2Storage::new(config)?)))
    } else {
        None
    };

    let broadcaster = Arc::new(Mutex::new(Broadcaster::init()));

    let telegram = Arc::new(telegram::TelegramBridge::init(
        broadcaster.clone(),
        config.clone(),
    ));
    let discord = Arc::new(
        discord::DiscordBridge::new(config.clone(), broadcaster.clone(), storage.clone()).await?,
    );

    {
        broadcaster
            .lock()
            .await
            .add_receiver(telegram.clone())
            .add_receiver(discord.clone());
    }

    tokio::join!(telegram.start(), discord.start());

    Ok(())
}

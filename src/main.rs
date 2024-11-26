use std::sync::Arc;

use broadcast::Broadcaster;
use color_eyre::{eyre::Result, Section};
use tokio::sync::Mutex;
use tracing::*;
use tracing_subscriber::{filter::LevelFilter, EnvFilter};

mod broadcast;
mod config;
mod core;
mod discord;
mod storage;
mod telegram;
pub use config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env()?
        .add_directive("oxibridge=debug".parse()?);

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .compact()
        .init();

    color_eyre::install()?;

    info!("hello, world!");

    debug!("reading config file");
    let config = String::from_utf8(tokio::fs::read("config.yml").await.suggestion(
        "Create a `config.yml` file and fill it out. Look at `config.example.yml` for reference.",
    )?)?;
    let config: Arc<Config> = Arc::new(serde_yaml::from_str(&config)?);

    // r2 storage
    let storage = match &config.shared.r2 {
        Some(config) => Some(Arc::new(Mutex::new(storage::R2Storage::new(config)?))),
        None => None,
    };

    let broadcaster = Arc::new(Mutex::new(Broadcaster::init()));

    let telegram = Arc::new(telegram::TelegramBridge::init(
        broadcaster.clone(),
        config.clone(),
    ));
    let discord_receiver = Arc::new(discord::DiscordBroadcastReceiver { storage });

    {
        broadcaster
            .lock()
            .await
            .add_receiver(telegram.clone())
            .add_receiver(discord_receiver);
    }

    tokio::join!(
        telegram.start(),
        discord::start(broadcaster, config.clone())
    );

    Ok(())
}

use std::sync::Arc;

use broadcast::Broadcaster;
use color_eyre::{eyre::Result, Section};
use tokio::sync::Mutex;
use tracing::*;
use tracing_subscriber::{
    fmt::writer::MakeWriterExt, layer::SubscriberExt, util::SubscriberInitExt,
};

mod broadcast;
mod config;
mod core;
mod discord;
mod telegram;
pub use config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .without_time()
                .with_writer(std::io::stdout.with_max_level(Level::DEBUG)),
        )
        .with(tracing_error::ErrorLayer::default())
        .init();

    color_eyre::install()?;

    info!("hello, world!");

    debug!("reading config file");
    let config = String::from_utf8(tokio::fs::read("config.yml").await.suggestion(
        "Create a `config.yml` file and fill it out. Look at `config.example.yml` for reference.",
    )?)?;
    let config: Arc<Config> = Arc::new(serde_yaml::from_str(&config)?);

    let broadcaster = Arc::new(Mutex::new(Broadcaster::init()));

    let telegram = Arc::new(telegram::TelegramBridge::init(
        broadcaster.clone(),
        config.clone(),
    ));
    let discord_receiver = Arc::new(discord::DiscordBroadcastReceiver);

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

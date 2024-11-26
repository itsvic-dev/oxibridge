use std::sync::Arc;

use broadcast::Broadcaster;
use color_eyre::Result;
use tokio::sync::Mutex;
use tracing::*;
use tracing_subscriber::{
    fmt::writer::MakeWriterExt, layer::SubscriberExt, util::SubscriberInitExt,
};

mod broadcast;
mod core;
mod discord;
mod telegram;

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

    let broadcaster = Arc::new(Mutex::new(Broadcaster::init()));

    let telegram = Arc::new(telegram::TelegramBridge::init(broadcaster.clone()));
    let discord_receiver = Arc::new(discord::DiscordBroadcastReceiver);

    {
        let mut broadcaster_locked = broadcaster.lock().await;
        broadcaster_locked.add_receiver(telegram.clone());
        broadcaster_locked.add_receiver(discord_receiver);
    }

    tokio::join!(telegram.start(), discord::start(broadcaster));

    Ok(())
}

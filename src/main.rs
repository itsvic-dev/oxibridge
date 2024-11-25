use color_eyre::Result;
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

    tokio::join!(telegram::start(), discord::start());

    Ok(())
}

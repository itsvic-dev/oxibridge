use color_eyre::Result;
use tracing::*;
use tracing_subscriber::{
    fmt::writer::MakeWriterExt, layer::SubscriberExt, util::SubscriberInitExt,
};

mod core;
mod telegram;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .without_time()
                .with_writer(std::io::stdout.with_max_level(Level::DEBUG)),
        )
        .init();

    color_eyre::install()?;

    info!("hello, world!");
    telegram::start().await;

    Ok(())
}

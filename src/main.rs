use color_eyre::{eyre::Result, Section};
use tokio::sync::broadcast::error::RecvError;
use tracing::*;
use tracing_subscriber::{filter::LevelFilter, layer::SubscriberExt, EnvFilter, Layer};

mod backends;
mod config;
mod core;
mod storage;
mod tasks;
pub use config::Config;

#[tokio::main]
async fn main() -> Result<()> {
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
        .map(|(group_name, group_backend_config)| {
            let (tx, _) = tokio::sync::broadcast::channel(32);

            let backends: Vec<_> = config
                .backends
                .iter()
                .map(|(name, backend)| {
                    backends::get_backend(
                        name,
                        backend,
                        group_name,
                        &group_backend_config[name],
                        tx.clone(),
                    )
                    .unwrap_or_else(|| panic!("Unsupported backend type \"{}\"", backend.kind))
                })
                .collect();

            (group_name, backends, tx)
        })
        .collect();

    for (group_name, backends, tx) in groups {
        let group_name = group_name.clone();
        let mut rx = tx.subscribe();
        info!("Bringing up backends for group '{}'", group_name);

        for backend in backends {
            backend.start().await;
        }
        // tasks::add_task(tokio::spawn(async move {
        //     loop {
        //         let msg = rx.recv().await;
        //         match msg {
        //             Err(e) => match e {
        //                 RecvError::Closed => {
        //                     error!("Channel closed for group '{}'", group_name);
        //                     break;
        //                 }
        //                 RecvError::Lagged(count) => {
        //                     warn!("Missed {} messages for group '{}'", count, group_name);
        //                 }
        //             },
        //             Ok(msg) => debug!(
        //                 "({}, {}): {:?}",
        //                 msg.group_name, msg.backend_name, msg.content
        //             ),
        //         }
        //     }
        // }));
    }

    futures::future::join_all(tasks::get_tasks()).await;

    Ok(())
}

fn setup_logging() -> Result<()> {
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

    Ok(())
}

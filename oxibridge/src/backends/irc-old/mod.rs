use crate::{
    broadcast::{Broadcaster, MessageEvent, Source},
    config::GroupConfig,
    irc::parsers::to_core_message,
    Config,
};
use color_eyre::Result;
use futures::prelude::*;
use irc::client::{prelude::Client, ClientStream};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::*;

mod broadcast;
mod parsers;

pub struct IrcBridge {
    client: Client,
    stream: Arc<Mutex<ClientStream>>,
    broadcaster: Arc<Mutex<Broadcaster>>,
    config: Arc<Config>,
}

impl IrcBridge {
    #[instrument(skip_all)]
    pub async fn new(config: Arc<Config>, broadcaster: Arc<Mutex<Broadcaster>>) -> Result<Self> {
        debug!("Creating IRC bot");

        let irc_config = match &config.shared.irc {
            Some(t) => t,
            None => {
                return Err(color_eyre::eyre::eyre!(
                    "IRC is not configured in shared config"
                ))
            }
        };

        let mut channels = Vec::new();
        for group in &config.groups {
            if let Some(channel) = group.irc_channel.clone() {
                channels.push(channel.to_owned());
            }
        }

        let irc_config = irc::client::prelude::Config {
            nickname: Some(irc_config.nickname.to_owned()),
            server: Some(irc_config.server.to_owned()),
            channels,
            use_tls: Some(irc_config.use_tls),
            port: Some(irc_config.port),
            ..irc::client::data::Config::default()
        };

        let mut client = Client::from_config(irc_config).await?;
        client.identify()?;
        let stream = Arc::new(Mutex::new(client.stream()?));

        Ok(Self {
            client,
            stream,
            broadcaster,
            config,
        })
    }

    pub async fn start(&self) {
        while let Ok(Some(message)) = self.stream.lock().await.next().await.transpose() {
            let group: Vec<GroupConfig> = self
                .config
                .groups
                .clone()
                .into_iter()
                .filter(|g| {
                    g.irc_channel
                        .as_ref()
                        .map(|ic| ic == message.response_target().unwrap_or(""))
                        .unwrap_or(false)
                })
                .collect();

            let group = match group.first() {
                Some(group) => group,
                None => continue,
            };

            let core_msg = match to_core_message(&message).await {
                Ok(cm) => cm,
                Err(e) => {
                    error!(?e, "Failed to parse into core message");
                    continue;
                }
            };

            debug!(?core_msg, "got core message");

            if let Err(why) = self
                .broadcaster
                .lock()
                .await
                .broadcast(group, &MessageEvent::Create(core_msg), Source::Irc)
                .await
            {
                error!(?why, "Failed to broadcast message");
            }
        }
    }
}

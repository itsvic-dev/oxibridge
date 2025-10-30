use std::error::Error;

use futures::{SinkExt, TryStreamExt};
use reqwest_websocket::{Message, RequestBuilderExt};
use tokio::{sync::broadcast, task::JoinHandle};

use crate::{
    Client,
    types::gateway::{self, GatewayEvent, Identify, IdentifyProperties, events::Event},
};

#[async_trait::async_trait]
pub trait Gateway {
    async fn start_gateway(
        &self,
        tx: broadcast::Sender<GatewayMessage>,
    ) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug, Clone)]
pub enum GatewayMessage {
    Ready(gateway::events::Ready),
}

#[async_trait::async_trait]
impl Gateway for Client {
    async fn start_gateway(
        &self,
        tx: broadcast::Sender<GatewayMessage>,
    ) -> Result<(), Box<dyn Error>> {
        let response = self
            .http
            .get("wss://gateway.discord.gg/?v=10&encoding=json")
            .upgrade()
            .send()
            .await?;

        let mut websocket = response.into_websocket().await?;
        let mut tasks: Vec<JoinHandle<()>> = vec![];

        while let Some(message) = websocket.try_next().await? {
            if let Message::Text(text) = message {
                let event: GatewayEvent = serde_json::from_str(&text)?;
                log::debug!("event received: {:?} ({:?})", event.opcode, event.event);

                match event.opcode {
                    gateway::Opcode::Hello => {
                        if let Some(data) = event.data {
                            // let data: gateway::Hello = serde_json::from_value(data)?;
                            websocket
                                .send(Message::Text(serde_json::to_string(&GatewayEvent {
                                    opcode: gateway::Opcode::Identify,
                                    data: Some(serde_json::to_value(&Identify {
                                        token: self.token.unsecure().to_owned(),
                                        properties: IdentifyProperties {
                                            os: "linux".to_owned(),
                                            browser: "minicord".to_owned(),
                                            device: "minicord".to_owned(),
                                        },
                                        intents: 1 << 9 | 1 << 15, // GUILD_MESSAGES, MESSAGE_CONTENT
                                    })?),
                                    sequence: None,
                                    event: None,
                                })?))
                                .await?;

                            // TODO: heartbeat
                            // tasks.push(tokio::spawn(async move {
                            //     loop {
                            //         tokio::time::sleep(Duration::from_millis(
                            //             data.heartbeat_interval,
                            //         ))
                            //         .await;

                            //         if let Err(_) =
                            //             websocket.send(Message::Text("".to_owned())).await
                            //         {
                            //             break;
                            //         }
                            //     }
                            // }));
                        }
                    }

                    gateway::Opcode::Dispatch => {
                        if let Some(event_type) = event.event {
                            match event_type {
                                Event::Ready => {
                                    if let Some(data) = event.data {
                                        let data: gateway::events::Ready =
                                            serde_json::from_value(data)?;

                                        tx.send(GatewayMessage::Ready(data))?;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        futures::future::join_all(tasks).await;

        Ok(())
    }
}

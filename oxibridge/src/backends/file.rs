use std::{error::Error, time::Duration};

use log::{debug, warn};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    sync::broadcast,
    time::sleep,
};

use crate::{
    backends::BackendGroup,
    config::{BackendConfig, GroupBackendConfig},
};

pub struct FileBackend {
    tx: broadcast::Sender<super::BackendMessage>,

    file_path: String,
    name: String,
    group_configs: Vec<BackendGroup>,
}

#[async_trait::async_trait]
impl super::Backend for FileBackend {
    fn new(name: &str, config: &BackendConfig, group_configs: &[BackendGroup]) -> Self {
        Self {
            tx,
            file_path: config.token.clone(),
            name: name.to_owned(),
            group_configs: group_configs.to_vec(),
        }
    }

    async fn start(&self) -> Result<(), Box<dyn Error>> {
        debug!(
            "FileBackend '{}' started, file: '{}'",
            self.name, self.file_path
        );

        if !self.group_config.readonly {
            let mut rx = self.tx.subscribe();
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.file_path)
                .await?;

            crate::tasks::add_task(tokio::spawn(async move {
                while let Ok(msg) = rx.recv().await {
                    let content = format!(
                        "({}, {}) {}: {}\n",
                        msg.group_name,
                        msg.backend_name,
                        msg.content.author.full_name(None),
                        msg.content.content
                    );
                    if file.write_all(content.as_bytes()).await.is_err() {
                        warn!("failed to write to file");
                    }
                }
            }));
        }

        if !self.group_config.writeonly {
            // read all lines from file and transmit them as messages
            let file = tokio::fs::OpenOptions::new()
                .read(true)
                .open(&self.file_path)
                .await?;

            let reader = tokio::io::BufReader::new(file);
            let mut lines = reader.lines();
            let group_name = self.group_name.clone();
            let name = self.name.clone();
            let tx = self.tx.clone();

            crate::tasks::add_task(tokio::spawn(async move {
                // wait for a second to let other backends start
                sleep(Duration::from_secs(1)).await;
                while let Ok(Some(line)) = lines.next_line().await {
                    let message = crate::core::Message::new(
                        crate::core::PartialAuthor {
                            display_name: None,
                            username: "file_backend".to_owned(),
                        }
                        .into(),
                        line,
                        vec![],
                        None,
                        None,
                    )
                    .await;
                    let backend_message = super::BackendMessage {
                        group_name: group_name.clone(),
                        backend_name: name.clone(),
                        content: message,
                    };
                    if tx.send(backend_message).is_err() {
                        warn!("failed to broadcast message");
                    }
                }
            }));
        }

        Ok(())
    }
}

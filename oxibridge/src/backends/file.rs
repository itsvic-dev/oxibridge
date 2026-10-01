use std::{error::Error, time::Duration};

use log::{debug, warn};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    time::sleep,
};

use crate::{
    backends::{BackendGroup, MessageEvent},
    config::BackendConfig,
};

pub struct FileBackend {
    file_path: String,
    name: String,
    group_configs: Vec<BackendGroup>,
}

#[async_trait::async_trait]
impl super::Backend for FileBackend {
    fn new(name: &str, config: &BackendConfig, group_configs: &[BackendGroup]) -> Self {
        Self {
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

        for group in &self.group_configs {
            if !group.config.readonly {
                let mut rx = group.subscribe();
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.file_path)
                    .await?;

                crate::tasks::add_task(tokio::spawn(async move {
                    while let Some(msg) = rx.recv().await {
                        let event = match msg.event {
                            MessageEvent::Create(message) => format!(
                                "{}: {}",
                                message.author.full_name(None),
                                message.content
                            ),
                            MessageEvent::Edit(message) => format!(
                                "[edit #{}] {}: {}",
                                message.id,
                                message.author.full_name(None),
                                message.content
                            ),
                            MessageEvent::Delete(id) => format!("[delete #{id}]"),
                        };
                        let content =
                            format!("({}, {}) {event}\n", msg.group_name, msg.backend_name);
                        if file.write_all(content.as_bytes()).await.is_err() {
                            warn!("failed to write to file");
                        }
                    }
                }))?;
            }

            if !group.config.writeonly {
                // read all lines from file and transmit them as messages
                let file = tokio::fs::OpenOptions::new()
                    .read(true)
                    .open(&self.file_path)
                    .await?;

                let reader = tokio::io::BufReader::new(file);
                let mut lines = reader.lines();
                let group = group.clone();

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
                        );
                        group.send(MessageEvent::Create(message));
                    }
                }))?;
            }
        }

        Ok(())
    }
}

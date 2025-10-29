use std::time::Duration;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    sync::broadcast,
    time::sleep,
};
use log::debug;

use crate::config::{BackendConfig, GroupBackendConfig};

pub struct FileBackend {
    tx: broadcast::Sender<super::BackendMessage>,

    file_path: String,
    name: String,
    group_name: String,
    group_config: GroupBackendConfig,
}

#[async_trait::async_trait]
impl super::Backend for FileBackend {
    fn new(
        name: &str,
        config: &BackendConfig,
        group_name: &str,
        group_config: &GroupBackendConfig,
        tx: broadcast::Sender<super::BackendMessage>,
    ) -> Self {
        FileBackend {
            tx,
            file_path: config.token.clone(),
            name: name.to_string(),
            group_name: group_name.to_string(),
            group_config: group_config.clone(),
        }
    }

    async fn start(&self) {
        debug!(
            "FileBackend '{}' for group '{}' started, file: '{}'",
            self.name, self.group_name, self.file_path
        );

        if !self.group_config.readonly {
            let mut rx = self.tx.subscribe();
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.file_path)
                .await
                .unwrap();
            crate::tasks::add_task(tokio::spawn(async move {
                while let Ok(msg) = rx.recv().await {
                    let content = format!(
                        "({}, {}) {}: {}\n",
                        msg.group_name,
                        msg.backend_name,
                        msg.content.author.full_name(None),
                        msg.content.content
                    );
                    file.write_all(content.as_bytes()).await.unwrap();
                }
            }));
        }

        if !self.group_config.writeonly {
            // read all lines from file and transmit them as messages
            let file = tokio::fs::OpenOptions::new()
                .read(true)
                .open(&self.file_path)
                .await
                .unwrap();

            let reader = tokio::io::BufReader::new(file);
            let mut lines = reader.lines();
            let group_name = self.group_name.clone();
            let name = self.name.clone();
            let tx = self.tx.clone();

            crate::tasks::add_task(tokio::spawn(async move {
                // wait for a second to let other backends start
                sleep(Duration::from_secs(1)).await;
                while let Some(line) = lines.next_line().await.unwrap() {
                    let message = crate::core::Message::new(
                        crate::core::PartialAuthor {
                            display_name: None,
                            username: "file_backend".to_string(),
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
                    tx.send(backend_message).unwrap();
                }
            }));
        }
    }
}

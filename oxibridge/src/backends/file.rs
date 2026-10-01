use std::{error::Error, path::PathBuf, time::Duration};

use log::{debug, warn};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    task::JoinSet,
    time::sleep,
};

use crate::{
    backends::{BackendGroup, MessageEvent},
    core::{Message, PartialAuthor},
    database::{Database, Link},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub path: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupConfig {}

pub struct FileBackend {
    file_path: PathBuf,
    name: String,
    group_configs: Vec<BackendGroup>,
    database: Database,
}

impl FileBackend {
    pub fn new(
        name: &str,
        config: &Config,
        group_configs: &[BackendGroup],
        database: Database,
    ) -> Result<Self, Box<dyn Error>> {
        for group in group_configs {
            group
                .config
                .options::<GroupConfig>()
                .map_err(|e| format!("backend '{name}' in group '{}': {e}", group.name))?;
        }

        Ok(Self {
            file_path: config.path.clone(),
            name: name.to_owned(),
            group_configs: group_configs.to_vec(),
            database,
        })
    }
}

#[async_trait::async_trait]
impl super::Backend for FileBackend {
    async fn start(&self, tasks: &mut JoinSet<()>) -> Result<(), Box<dyn Error>> {
        debug!(
            "FileBackend '{}' started, file: '{}'",
            self.name,
            self.file_path.display()
        );

        for group in &self.group_configs {
            if !group.config.readonly {
                let mut rx = group.subscribe();
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.file_path)
                    .await?;

                tasks.spawn(async move {
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
                });
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
                let database = self.database.clone();
                let name = self.name.clone();
                let chat = self.file_path.display().to_string();

                tasks.spawn(async move {
                    // wait for a second to let other backends start
                    sleep(Duration::from_secs(1)).await;
                    let mut line_number = 0_u64;
                    while let Ok(Some(line)) = lines.next_line().await {
                        line_number = line_number.saturating_add(1);
                        let link = Link {
                            backend: name.clone(),
                            chat: chat.clone(),
                            platform_id: line_number.to_string(),
                        };
                        let id = match record(&database, &group.name, &link).await {
                            Ok(id) => id,
                            Err(e) => {
                                warn!("failed to record line {line_number}: {e}");
                                continue;
                            }
                        };
                        let message = Message {
                            id,
                            author: PartialAuthor {
                                display_name: None,
                                username: "file_backend".to_owned(),
                            }
                            .into(),
                            content: line,
                            attachments: vec![],
                            in_reply_to: None,
                            reply_author: None,
                        };
                        group.send(MessageEvent::Create(message));
                    }
                });
            }
        }

        Ok(())
    }
}

async fn record(database: &Database, group: &str, link: &Link) -> sqlx::Result<i64> {
    let id = database.create_message(group).await?;
    database.add_link(id, link).await?;
    Ok(id)
}

use std::sync::{Arc, LazyLock};

use async_tempfile::TempFile;
use tokio::sync::Mutex;

use crate::broadcast::Source;

#[derive(Debug)]
pub struct Author {
    pub display_name: Option<String>,
    pub username: String,
    pub avatar: Option<TempFile>,
    pub source: Source,
}

impl Author {
    pub fn full_name(&self, length: Option<usize>) -> String {
        let length = length.unwrap_or(32);

        let source: &str= match self.source {
            Source::Discord => "dc",
            Source::Telegram => "tg",
        };

        if let Some(display_name) = &self.display_name {
            let full_name = format!("{} (@{}/{})", display_name, source, self.username);

            if length != 0 && full_name.len() > length {
                display_name.clone()
            } else {
                full_name
            }
        } else {
            self.username.clone()
        }
    }
}

#[derive(Debug)]
pub struct Message {
    pub author: Arc<Author>,
    pub content: String,
    pub attachments: Vec<Attachment>,
    pub id: u64,
    pub in_reply_to: Option<u64>,
    // FIXME: replace Arc<Author> with PartialAuthor
    pub reply_author: Option<Arc<Author>>,
}

static NEXT_ID: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(0));

impl Message {
    pub async fn new(
        author: Arc<Author>,
        content: String,
        attachments: Vec<Attachment>,
        in_reply_to: Option<u64>,
        reply_author: Option<Arc<Author>>,
    ) -> Self {
        let mut next_id = NEXT_ID.lock().await;
        let id = *next_id;
        *next_id += 1;

        Self {
            id,
            author,
            content,
            attachments,
            in_reply_to,
            reply_author,
        }
    }
}

#[derive(Debug)]
pub struct Attachment {
    pub file: TempFile,
    pub filename: String,
    pub spoilered: bool,
}

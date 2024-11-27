use std::sync::LazyLock;

use async_tempfile::TempFile;
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct Author {
    pub display_name: Option<String>,
    pub username: String,
    pub avatar: Option<TempFile>,
}

impl Author {
    pub fn full_name(&self) -> String {
        if let Some(display_name) = &self.display_name {
            let full_name = format!("{} ({})", display_name, self.username);
            if full_name.len() > 32 {
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
    pub author: Author,
    pub content: String,
    pub attachments: Vec<Attachment>,
    pub id: u64,
}

static NEXT_ID: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(0));

impl Message {
    pub async fn new(author: Author, content: String, attachments: Vec<Attachment>) -> Self {
        let mut next_id = NEXT_ID.lock().await;
        let id = *next_id;
        *next_id += 1;

        Self {
            id,
            author,
            content,
            attachments,
        }
    }
}

#[derive(Debug)]
pub struct Attachment {
    pub file: TempFile,
    pub spoilered: bool,
}

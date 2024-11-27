use async_tempfile::TempFile;

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
}

#[derive(Debug)]
pub struct Attachment {
    pub file: TempFile,
    pub spoilered: bool,
}

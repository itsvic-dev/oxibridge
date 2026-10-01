
#[derive(Debug, Clone)]
pub struct Author {
    pub display_name: Option<String>,
    pub username: String,
    // pub avatar: Option<TempFile>,
    // pub source: Source,
}

impl Author {
    pub fn full_name(&self, length: Option<usize>) -> String {
        let length = length.unwrap_or(32);

        // let source: &str = match self.source {
        //     Source::Discord => "dc",
        //     Source::Telegram => "tg",
        // };
        let source = "TODO";

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

#[derive(Debug, Clone)]
pub struct PartialAuthor {
    pub display_name: Option<String>,
    pub username: String,
    // pub source: Source,
}

impl From<&Author> for PartialAuthor {
    fn from(value: &Author) -> Self {
        Self {
            display_name: value.display_name.clone(),
            username: value.username.clone(),
            // source: value.source.clone(),
        }
    }
}

impl From<PartialAuthor> for Author {
    fn from(value: PartialAuthor) -> Self {
        Self {
            display_name: value.display_name.clone(),
            username: value.username.clone(),
            // source: value.source.clone(),
            // avatar: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub author: Author,
    pub content: String,
    pub attachments: Vec<Attachment>,
    /// Core message ID from [`crate::database::Database::create_message`].
    pub id: i64,
    pub in_reply_to: Option<i64>,
    pub reply_author: Option<PartialAuthor>,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    // pub file: TempFile,
    pub filename: String,
    pub spoilered: bool,
}

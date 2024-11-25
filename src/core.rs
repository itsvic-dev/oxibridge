#[derive(Debug)]
pub struct Author {
    pub display_name: String,
    pub username: String,
    pub avatar: Option<File>,
}

#[derive(Debug)]
pub struct Message {
    pub author: Author,
    pub content: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub file: File,
    pub spoilered: bool,
}

#[derive(Debug, Clone)]
pub struct File {
    pub name: String,
    pub bytes: Box<[u8]>,
}

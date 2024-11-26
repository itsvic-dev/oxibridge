use color_eyre::Result;
use std::{env::temp_dir, fs, path::PathBuf};
use tracing::debug;

#[derive(Debug)]
pub struct Author {
    pub display_name: Option<String>,
    pub username: String,
    pub avatar: Option<File>,
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

#[derive(Debug, Clone)]
pub struct Attachment {
    pub file: File,
    pub spoilered: bool,
}

#[derive(Debug, Clone)]
pub struct File {
    pub name: String,
    pub path: PathBuf,
}

impl Drop for File {
    fn drop(&mut self) {
        debug!("removing file {:?}", &self.path);
        fs::remove_file(&self.path).expect("failed to remove file");
    }
}

pub fn get_tmp_dir() -> Result<PathBuf> {
    let path = temp_dir().join("oxibridge");

    if !fs::exists(&path)? {
        fs::create_dir_all(&path)?;
    }

    Ok(path.into())
}

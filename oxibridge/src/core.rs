
/// The kind of platform a message or author comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    File,
    Irc,
}

impl Source {
    /// Short tag shown in author names, for example `irc` in `nick (@irc/nick)`.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Irc => "irc",
        }
    }

    /// The inverse of [`Self::tag`].
    pub fn from_tag(tag: &str) -> Option<Self> {
        [Self::File, Self::Irc]
            .into_iter()
            .find(|source| source.tag() == tag)
    }
}

#[derive(Debug, Clone)]
pub struct Author {
    pub display_name: Option<String>,
    pub username: String,
    // pub avatar: Option<TempFile>,
    pub source: Source,
}

impl Author {
    /// Formats the author as `display name (@source/username)`.
    ///
    /// Falls back to the display name alone if the result is longer than `length` (default 32, 0 for no limit).
    pub fn full_name(&self, length: Option<usize>) -> String {
        full_name(
            self.display_name.as_deref(),
            &self.username,
            self.source,
            length,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialAuthor {
    pub display_name: Option<String>,
    pub username: String,
    pub source: Source,
}

impl PartialAuthor {
    /// See [`Author::full_name`].
    pub fn full_name(&self, length: Option<usize>) -> String {
        full_name(
            self.display_name.as_deref(),
            &self.username,
            self.source,
            length,
        )
    }
}

fn full_name(
    display_name: Option<&str>,
    username: &str,
    source: Source,
    length: Option<usize>,
) -> String {
    let length = length.unwrap_or(32);

    if let Some(display_name) = display_name {
        let full_name = format!("{display_name} (@{}/{username})", source.tag());

        if length != 0 && full_name.len() > length {
            display_name.to_owned()
        } else {
            full_name
        }
    } else {
        username.to_owned()
    }
}

impl From<&Author> for PartialAuthor {
    fn from(value: &Author) -> Self {
        Self {
            display_name: value.display_name.clone(),
            username: value.username.clone(),
            source: value.source,
        }
    }
}

impl From<PartialAuthor> for Author {
    fn from(value: PartialAuthor) -> Self {
        Self {
            display_name: value.display_name,
            username: value.username,
            source: value.source,
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

#[cfg(test)]
mod tests {
    use super::{Author, Source};

    fn author(display_name: Option<&str>) -> Author {
        Author {
            display_name: display_name.map(str::to_owned),
            username: "nick".to_owned(),
            source: Source::Irc,
        }
    }

    #[test]
    fn includes_source_and_username() {
        assert_eq!(author(Some("Nick")).full_name(None), "Nick (@irc/nick)");
    }

    #[test]
    fn falls_back_to_display_name_when_too_long() {
        assert_eq!(author(Some("Nick")).full_name(Some(10)), "Nick");
    }

    #[test]
    fn has_no_length_limit_at_zero() {
        let name = "a".repeat(40);
        assert_eq!(
            author(Some(&name)).full_name(Some(0)),
            format!("{name} (@irc/nick)")
        );
    }

    #[test]
    fn uses_username_without_display_name() {
        assert_eq!(author(None).full_name(None), "nick");
    }
}

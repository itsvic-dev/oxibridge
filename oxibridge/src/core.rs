use std::sync::Arc;

use async_tempfile::TempFile;

/// The kind of platform a message or author comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    File,
    Irc,
    Telegram,
    Discord,
}

impl Source {
    /// Short tag shown in author names, for example `irc` in `nick (@irc/nick)`.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Irc => "irc",
            Self::Telegram => "tg",
            Self::Discord => "dc",
        }
    }

    /// The inverse of [`Self::tag`].
    pub fn from_tag(tag: &str) -> Option<Self> {
        [Self::File, Self::Irc, Self::Telegram, Self::Discord]
            .into_iter()
            .find(|source| source.tag() == tag)
    }

    /// Reads a mention like `@dc/name` at the start of `text`, and returns its length in bytes and the name.
    pub fn mention_at(self, text: &str) -> Option<(usize, &str)> {
        let rest = text
            .strip_prefix('@')?
            .strip_prefix(self.tag())?
            .strip_prefix('/')?;
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .unwrap_or(rest.len());
        // a mention at the end of a sentence keeps its full stop out
        let name = rest.get(..end)?.trim_end_matches('.');
        (!name.is_empty()).then(|| (text.len() - rest.len() + name.len(), name))
    }

    /// Finds all mentions like `@dc/name` in `text`, as byte ranges with the name.
    pub fn mentions(self, text: &str) -> Vec<(std::ops::Range<usize>, &str)> {
        let mut found = vec![];
        let mut previous: Option<char> = None;
        for (index, c) in text.char_indices() {
            let after_word = previous.is_some_and(char::is_alphanumeric);
            if c == '@'
                && !after_word
                && let Some((length, name)) =
                    text.get(index..).and_then(|rest| self.mention_at(rest))
            {
                found.push((index..index + length, name));
            }
            previous = Some(c);
        }
        found
    }
}

#[derive(Debug, Clone)]
pub struct Author {
    pub display_name: Option<String>,
    pub username: String,
    pub avatar: Option<Avatar>,
    pub source: Source,
}

#[derive(Debug, Clone)]
pub enum Avatar {
    /// A public URL that other services can fetch.
    Url(String),
    /// A JPEG file that has to be uploaded somewhere first.
    File(Arc<TempFile>),
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
            avatar: None,
            source: value.source,
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
    pub reactions: Vec<Reaction>,
}

impl Message {
    /// Summarizes the reactions made outside `backend`, for example `👍 2 · ❤️ 1`.
    ///
    /// Returns [`None`] if there are none.
    pub fn reaction_summary(&self, backend: &str) -> Option<String> {
        let mut totals: Vec<(&str, i64)> = vec![];
        for reaction in self.reactions.iter().filter(|r| r.backend != backend) {
            match totals
                .iter_mut()
                .find(|(emoji, _)| *emoji == reaction.emoji)
            {
                Some((_, count)) => *count = count.saturating_add(reaction.count),
                None => totals.push((&reaction.emoji, reaction.count)),
            }
        }
        if totals.is_empty() {
            return None;
        }
        totals.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
        Some(
            totals
                .iter()
                .map(|(emoji, count)| format!("{emoji} {count}"))
                .collect::<Vec<_>>()
                .join(" · "),
        )
    }
}

/// How many times a message got one emoji as a reaction on one backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaction {
    pub backend: String,
    /// A Unicode emoji, or the name of a custom emoji like `:blobcat:`.
    pub emoji: String,
    pub count: i64,
}

/// A file attached to a message. The file is deleted once the last copy of the attachment is dropped.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub file: Arc<TempFile>,
    /// Name to show and send the file as. Backends also use its extension to pick the kind of media.
    pub filename: String,
    pub spoilered: bool,
}

impl Attachment {
    /// Whether the file should be shown as an image, judging by its extension.
    pub fn is_image(&self) -> bool {
        let extension = std::path::Path::new(&self.filename)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        matches!(
            extension.as_deref(),
            Some("png" | "jpg" | "jpeg" | "webp" | "gif")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Author, Source};

    fn author(display_name: Option<&str>) -> Author {
        Author {
            display_name: display_name.map(str::to_owned),
            username: "nick".to_owned(),
            avatar: None,
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

    async fn attachment(filename: &str) -> Result<super::Attachment, async_tempfile::Error> {
        Ok(super::Attachment {
            file: std::sync::Arc::new(async_tempfile::TempFile::new().await?),
            filename: filename.to_owned(),
            spoilered: false,
        })
    }

    #[tokio::test]
    async fn recognizes_images_by_extension() -> Result<(), async_tempfile::Error> {
        assert!(attachment("photo.JPG").await?.is_image());
        assert!(!attachment("clip.mp4").await?.is_image());
        assert!(!attachment("noextension").await?.is_image());
        Ok(())
    }

    fn with_reactions(reactions: &[(&str, &str, i64)]) -> super::Message {
        super::Message {
            author: author(None),
            content: String::new(),
            attachments: vec![],
            id: 1,
            in_reply_to: None,
            reply_author: None,
            reactions: reactions
                .iter()
                .map(|&(backend, emoji, count)| super::Reaction {
                    backend: backend.to_owned(),
                    emoji: emoji.to_owned(),
                    count,
                })
                .collect(),
        }
    }

    #[test]
    fn sums_reactions_from_other_backends_by_count() {
        let message = with_reactions(&[("tg", "❤️", 1), ("dc", "👍", 1), ("tg", "👍", 1)]);
        assert_eq!(
            message.reaction_summary("irc").as_deref(),
            Some("👍 2 · ❤️ 1")
        );
    }

    #[test]
    fn leaves_out_reactions_from_the_target_backend() {
        let message = with_reactions(&[("tg", "❤️", 1)]);
        assert_eq!(message.reaction_summary("tg"), None);
    }

    #[test]
    fn finds_mentions_of_one_platform() {
        let text = "hi @dc/vic and @tg/bob, ask @dc/a.b.";
        let found = Source::Discord.mentions(text);
        let names: Vec<_> = found.iter().map(|(_, name)| *name).collect();
        assert_eq!(names, vec!["vic", "a.b"]);
        assert_eq!(
            found.first().and_then(|(range, _)| text.get(range.clone())),
            Some("@dc/vic")
        );
    }

    #[test]
    fn ignores_mentions_inside_words() {
        assert!(Source::Discord.mentions("mail@dc/vic @dc/").is_empty());
    }

    #[test]
    fn uses_username_without_display_name() {
        assert_eq!(author(None).full_name(None), "nick");
    }
}

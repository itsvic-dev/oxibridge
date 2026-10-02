use std::path::Path;

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

use crate::core::{Message, PartialAuthor, Reaction, Source};

/// Identifies one platform message that a core message was bridged as.
///
/// `chat` and `platform_id` are in whatever text form the backend uses for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub backend: String,
    pub chat: String,
    pub platform_id: String,
}

/// Persistent store of core message IDs and the platform messages they map to.
#[derive(Debug, Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Opens the database at `path`. Creates it and applies pending migrations if needed.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or migrated.
    pub async fn open(path: &Path) -> sqlx::Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        Self::connect(SqlitePoolOptions::new(), options).await
    }

    async fn connect(
        pool_options: SqlitePoolOptions,
        options: SqliteConnectOptions,
    ) -> sqlx::Result<Self> {
        let pool = pool_options.connect_with(options).await?;
        sqlx::migrate!().run(&pool).await?;
        Ok(Self { pool })
    }

    /// Opens an empty database that only lives as long as the returned value.
    #[cfg(test)]
    pub async fn in_memory() -> sqlx::Result<Self> {
        // every connection to an in-memory database gets its own database, so keep exactly one alive
        let pool_options = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None);
        Self::connect(pool_options, "sqlite::memory:".parse()?).await
    }

    /// Records `message`, which `backend` received in `group`, and returns its new core ID.
    ///
    /// The ID of `message` itself is ignored.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn create_message(
        &self,
        group: &str,
        backend: &str,
        message: &Message,
    ) -> sqlx::Result<i64> {
        let source = message.author.source.tag();
        sqlx::query_scalar!(
            r#"INSERT INTO messages
            (group_name, backend, author_username, author_display_name, author_source, content, in_reply_to)
            VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id AS "id!""#,
            group,
            backend,
            message.author.username,
            message.author.display_name,
            source,
            message.content,
            message.in_reply_to
        )
        .fetch_one(&self.pool)
        .await
    }

    /// Replaces the content of core message `message_id` after an edit.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn set_content(&self, message_id: i64, content: &str) -> sqlx::Result<()> {
        sqlx::query!(
            "UPDATE messages SET content = ? WHERE id = ?",
            content,
            message_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Loads core message `message_id` with its reactions, but without attachments.
    ///
    /// Returns [`None`] for unknown messages and messages recorded before content was stored.
    ///
    /// # Errors
    /// Returns an error if a query fails.
    pub async fn message(&self, message_id: i64) -> sqlx::Result<Option<Message>> {
        let Some(row) = sqlx::query!(
            "SELECT content, in_reply_to FROM messages WHERE id = ?",
            message_id
        )
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let (Some(content), Some(author)) = (row.content, self.message_author(message_id).await?)
        else {
            return Ok(None);
        };
        let reply_author = match row.in_reply_to {
            Some(reply) => self.message_author(reply).await?,
            None => None,
        };
        Ok(Some(Message {
            author: author.into(),
            content,
            attachments: vec![],
            id: message_id,
            in_reply_to: row.in_reply_to,
            reply_author,
            reactions: self.reactions(message_id).await?,
        }))
    }

    /// Returns the backend that core message `message_id` was first received on.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn message_origin(&self, message_id: i64) -> sqlx::Result<Option<String>> {
        Ok(
            sqlx::query_scalar!("SELECT backend FROM messages WHERE id = ?", message_id)
                .fetch_optional(&self.pool)
                .await?
                .flatten(),
        )
    }

    /// Lists the reactions to core message `message_id`.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn reactions(&self, message_id: i64) -> sqlx::Result<Vec<Reaction>> {
        sqlx::query_as!(
            Reaction,
            "SELECT backend, emoji, count FROM reactions WHERE message_id = ? ORDER BY rowid",
            message_id
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Replaces all reactions that core message `message_id` has on `backend`.
    ///
    /// # Errors
    /// Returns an error if a query fails.
    pub async fn set_reactions(
        &self,
        message_id: i64,
        backend: &str,
        reactions: &[(String, i64)],
    ) -> sqlx::Result<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query!(
            "DELETE FROM reactions WHERE message_id = ? AND backend = ?",
            message_id,
            backend
        )
        .execute(&mut *transaction)
        .await?;
        for (emoji, count) in reactions.iter().filter(|(_, count)| *count > 0) {
            sqlx::query!(
                "INSERT INTO reactions (message_id, backend, emoji, count) VALUES (?, ?, ?, ?)",
                message_id,
                backend,
                emoji,
                count
            )
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await
    }

    /// Changes the count of one reaction to core message `message_id` on `backend` by `delta`.
    ///
    /// # Errors
    /// Returns an error if a query fails.
    pub async fn add_reaction(
        &self,
        message_id: i64,
        backend: &str,
        emoji: &str,
        delta: i64,
    ) -> sqlx::Result<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query!(
            "INSERT INTO reactions (message_id, backend, emoji, count) VALUES (?, ?, ?, ?)
            ON CONFLICT (message_id, backend, emoji) DO UPDATE SET count = count + excluded.count",
            message_id,
            backend,
            emoji,
            delta
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query!(
            "DELETE FROM reactions WHERE message_id = ? AND count <= 0",
            message_id
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await
    }

    /// Returns the author of core message `message_id`, if it is known.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn message_author(&self, message_id: i64) -> sqlx::Result<Option<PartialAuthor>> {
        let row = sqlx::query!(
            "SELECT author_username, author_display_name, author_source FROM messages WHERE id = ?",
            message_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.and_then(|row| {
            Some(PartialAuthor {
                username: row.author_username?,
                display_name: row.author_display_name,
                source: Source::from_tag(&row.author_source?)?,
            })
        }))
    }

    /// Records that core message `message_id` exists as `link`.
    ///
    /// # Errors
    /// Returns an error if `link` is already recorded, or if the query fails.
    pub async fn add_link(&self, message_id: i64, link: &Link) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO message_links (message_id, backend, chat, platform_id) VALUES (?, ?, ?, ?)",
            message_id,
            link.backend,
            link.chat,
            link.platform_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Finds the core message that `link` belongs to.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn find_message(&self, link: &Link) -> sqlx::Result<Option<i64>> {
        sqlx::query_scalar!(
            "SELECT message_id FROM message_links WHERE backend = ? AND chat = ? AND platform_id = ?",
            link.backend,
            link.chat,
            link.platform_id
        )
        .fetch_optional(&self.pool)
        .await
    }

    /// Lists the platform messages on `backend` for core message `message_id`, in the order they were added.
    ///
    /// There can be more than one, for example when a backend splits a long message.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn links(&self, message_id: i64, backend: &str) -> sqlx::Result<Vec<Link>> {
        sqlx::query_as!(
            Link,
            "SELECT backend, chat, platform_id FROM message_links WHERE message_id = ? AND backend = ? ORDER BY rowid",
            message_id,
            backend
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Finds the core messages that have `platform_id` on `backend` in any chat, with that chat.
    ///
    /// Used for platforms that report some events without the chat, like deletes in Telegram basic groups.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn find_messages_in_any_chat(
        &self,
        backend: &str,
        platform_id: &str,
    ) -> sqlx::Result<Vec<(i64, String)>> {
        let rows = sqlx::query!(
            "SELECT message_id, chat FROM message_links WHERE backend = ? AND platform_id = ?",
            backend,
            platform_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| (row.message_id, row.chat))
            .collect())
    }

    /// Returns all state values that `backend` stored with [`Self::set_state`], as `(key, value)` pairs.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn state(&self, backend: &str) -> sqlx::Result<Vec<(String, String)>> {
        let rows = sqlx::query!(
            "SELECT key, value FROM backend_state WHERE backend = ? ORDER BY key",
            backend
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|row| (row.key, row.value)).collect())
    }

    /// Stores `value` under `key` for `backend`, replacing any earlier value.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn set_state(&self, backend: &str, key: &str, value: &str) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO backend_state (backend, key, value) VALUES (?, ?, ?)
            ON CONFLICT (backend, key) DO UPDATE SET value = excluded.value",
            backend,
            key,
            value
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Forgets the platform messages on `backend` for core message `message_id`.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn remove_links(&self, message_id: i64, backend: &str) -> sqlx::Result<()> {
        sqlx::query!(
            "DELETE FROM message_links WHERE message_id = ? AND backend = ?",
            message_id,
            backend
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Database, Link};
    use crate::core::{Message, PartialAuthor, Reaction, Source};

    fn author() -> PartialAuthor {
        PartialAuthor {
            display_name: Some("Vic".to_owned()),
            username: "vic".to_owned(),
            source: Source::Irc,
        }
    }

    fn message() -> Message {
        Message {
            author: author().into(),
            content: "hi".to_owned(),
            attachments: vec![],
            id: 0,
            in_reply_to: None,
            reply_author: None,
            reactions: vec![],
        }
    }

    fn reaction(backend: &str, emoji: &str, count: i64) -> Reaction {
        Reaction {
            backend: backend.to_owned(),
            emoji: emoji.to_owned(),
            count,
        }
    }

    #[tokio::test]
    async fn loads_a_message_with_its_reply_and_edits() -> sqlx::Result<()> {
        let db = database().await?;
        let first = db.create_message("g", "irc", &message()).await?;
        let reply = Message {
            in_reply_to: Some(first),
            ..message()
        };
        let second = db.create_message("g", "irc", &reply).await?;
        db.set_content(second, "edited").await?;

        let loaded = db.message(second).await?.ok_or(sqlx::Error::RowNotFound)?;
        assert_eq!(loaded.id, second);
        assert_eq!(loaded.content, "edited");
        assert_eq!(loaded.in_reply_to, Some(first));
        assert_eq!(loaded.reply_author, Some(author()));
        assert_eq!(db.message_origin(second).await?, Some("irc".to_owned()));
        Ok(())
    }

    #[tokio::test]
    async fn counts_reactions_up_and_down() -> sqlx::Result<()> {
        let db = database().await?;
        let id = db.create_message("g", "irc", &message()).await?;
        db.add_reaction(id, "tg", "👍", 1).await?;
        db.add_reaction(id, "tg", "👍", 1).await?;
        db.add_reaction(id, "tg", "❤️", 1).await?;
        db.add_reaction(id, "tg", "❤️", -1).await?;
        assert_eq!(db.reactions(id).await?, vec![reaction("tg", "👍", 2)]);
        Ok(())
    }

    #[tokio::test]
    async fn replaces_the_reactions_of_one_backend() -> sqlx::Result<()> {
        let db = database().await?;
        let id = db.create_message("g", "irc", &message()).await?;
        db.add_reaction(id, "tg", "👍", 1).await?;
        db.add_reaction(id, "dc", "👍", 1).await?;
        db.set_reactions(id, "dc", &[("🎉".to_owned(), 3), ("😢".to_owned(), 0)])
            .await?;
        assert_eq!(
            db.reactions(id).await?,
            vec![reaction("tg", "👍", 1), reaction("dc", "🎉", 3)]
        );
        Ok(())
    }

    async fn database() -> sqlx::Result<Database> {
        Database::in_memory().await
    }

    fn link(backend: &str, chat: &str, platform_id: &str) -> Link {
        Link {
            backend: backend.to_owned(),
            chat: chat.to_owned(),
            platform_id: platform_id.to_owned(),
        }
    }

    #[tokio::test]
    async fn gives_each_message_a_new_id() -> sqlx::Result<()> {
        let db = database().await?;
        let first = db.create_message("g", "irc", &message()).await?;
        let second = db.create_message("g", "irc", &message()).await?;
        assert_ne!(first, second);
        Ok(())
    }

    #[tokio::test]
    async fn finds_the_message_of_a_link() -> sqlx::Result<()> {
        let db = database().await?;
        let id = db.create_message("g", "irc", &message()).await?;
        db.add_link(id, &link("tg", "-100", "5")).await?;
        assert_eq!(db.find_message(&link("tg", "-100", "5")).await?, Some(id));
        Ok(())
    }

    #[tokio::test]
    async fn finds_nothing_for_an_unknown_link() -> sqlx::Result<()> {
        let db = database().await?;
        let id = db.create_message("g", "irc", &message()).await?;
        db.add_link(id, &link("tg", "-100", "5")).await?;
        assert_eq!(db.find_message(&link("tg", "-200", "5")).await?, None);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_a_platform_message_linked_twice() -> sqlx::Result<()> {
        let db = database().await?;
        let first = db.create_message("g", "irc", &message()).await?;
        let second = db.create_message("g", "irc", &message()).await?;
        db.add_link(first, &link("tg", "-100", "5")).await?;
        assert!(db.add_link(second, &link("tg", "-100", "5")).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn lists_links_of_one_backend_in_order() -> sqlx::Result<()> {
        let db = database().await?;
        let id = db.create_message("g", "irc", &message()).await?;
        db.add_link(id, &link("dc", "1", "20")).await?;
        db.add_link(id, &link("tg", "-100", "5")).await?;
        db.add_link(id, &link("dc", "1", "10")).await?;
        assert_eq!(
            db.links(id, "dc").await?,
            vec![link("dc", "1", "20"), link("dc", "1", "10")]
        );
        Ok(())
    }

    #[tokio::test]
    async fn remembers_the_author_of_a_message() -> sqlx::Result<()> {
        let db = database().await?;
        let id = db.create_message("g", "irc", &message()).await?;
        assert_eq!(db.message_author(id).await?, Some(author()));
        Ok(())
    }

    #[tokio::test]
    async fn has_no_author_for_an_unknown_message() -> sqlx::Result<()> {
        let db = database().await?;
        assert_eq!(db.message_author(42).await?, None);
        Ok(())
    }

    #[tokio::test]
    async fn finds_messages_without_knowing_the_chat() -> sqlx::Result<()> {
        let db = database().await?;
        let id = db.create_message("g", "irc", &message()).await?;
        db.add_link(id, &link("tg", "-100", "5")).await?;
        db.add_link(id, &link("dc", "1", "5")).await?;
        assert_eq!(
            db.find_messages_in_any_chat("tg", "5").await?,
            vec![(id, "-100".to_owned())]
        );
        Ok(())
    }

    #[tokio::test]
    async fn keeps_the_latest_state_value_per_backend() -> sqlx::Result<()> {
        let db = database().await?;
        db.set_state("tg", "home_dc", "1").await?;
        db.set_state("tg", "home_dc", "2").await?;
        db.set_state("other", "home_dc", "3").await?;
        assert_eq!(
            db.state("tg").await?,
            vec![("home_dc".to_owned(), "2".to_owned())]
        );
        Ok(())
    }

    #[tokio::test]
    async fn removes_the_links_of_one_backend() -> sqlx::Result<()> {
        let db = database().await?;
        let id = db.create_message("g", "irc", &message()).await?;
        db.add_link(id, &link("tg", "-100", "5")).await?;
        db.add_link(id, &link("dc", "1", "7")).await?;
        db.remove_links(id, "tg").await?;
        assert_eq!(db.find_message(&link("tg", "-100", "5")).await?, None);
        assert_eq!(db.links(id, "dc").await?, vec![link("dc", "1", "7")]);
        Ok(())
    }
}

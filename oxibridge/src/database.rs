use std::path::Path;

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

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

    /// Creates a core message in `group` and returns its ID.
    ///
    /// # Errors
    /// Returns an error if the query fails.
    pub async fn create_message(&self, group: &str) -> sqlx::Result<i64> {
        sqlx::query_scalar!(
            r#"INSERT INTO messages (group_name) VALUES (?) RETURNING id AS "id!""#,
            group
        )
        .fetch_one(&self.pool)
        .await
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
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    use super::{Database, Link};

    async fn database() -> sqlx::Result<Database> {
        // every connection to an in-memory database gets its own database, so keep exactly one alive
        let pool_options = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None);
        Database::connect(
            pool_options,
            SqliteConnectOptions::from_str("sqlite::memory:")?,
        )
        .await
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
        let first = db.create_message("g").await?;
        let second = db.create_message("g").await?;
        assert_ne!(first, second);
        Ok(())
    }

    #[tokio::test]
    async fn finds_the_message_of_a_link() -> sqlx::Result<()> {
        let db = database().await?;
        let id = db.create_message("g").await?;
        db.add_link(id, &link("tg", "-100", "5")).await?;
        assert_eq!(db.find_message(&link("tg", "-100", "5")).await?, Some(id));
        Ok(())
    }

    #[tokio::test]
    async fn finds_nothing_for_an_unknown_link() -> sqlx::Result<()> {
        let db = database().await?;
        let id = db.create_message("g").await?;
        db.add_link(id, &link("tg", "-100", "5")).await?;
        assert_eq!(db.find_message(&link("tg", "-200", "5")).await?, None);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_a_platform_message_linked_twice() -> sqlx::Result<()> {
        let db = database().await?;
        let first = db.create_message("g").await?;
        let second = db.create_message("g").await?;
        db.add_link(first, &link("tg", "-100", "5")).await?;
        assert!(db.add_link(second, &link("tg", "-100", "5")).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn lists_links_of_one_backend_in_order() -> sqlx::Result<()> {
        let db = database().await?;
        let id = db.create_message("g").await?;
        db.add_link(id, &link("dc", "1", "20")).await?;
        db.add_link(id, &link("tg", "-100", "5")).await?;
        db.add_link(id, &link("dc", "1", "10")).await?;
        assert_eq!(
            db.links(id, "dc").await?,
            vec![link("dc", "1", "20"), link("dc", "1", "10")]
        );
        Ok(())
    }
}

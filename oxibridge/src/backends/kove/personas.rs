//! Bridged identities: the personas that the bot posts as, one per remote user.

use std::collections::HashMap;

use bytes::Bytes;
use kove::{
    Client, Id,
    model::{BridgedIdentity, CreateIdentity, UpdateIdentity, UploadPurpose},
};
use log::warn;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{
    core::{Author, Avatar, Source},
    database::Database,
};

use super::TaskResult;

const STATE_PREFIX: &str = "persona:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Persona {
    id: String,
    display_name: String,
    remote_username: Option<String>,
    profile_url: Option<String>,
    // the avatar URL, or a hash of the avatar file
    avatar: Option<String>,
}

/// Creates and updates the personas of a bot, and remembers them in the database.
pub struct Personas {
    backend: String,
    client: Client,
    database: Database,
    // keyed by state key, which has the platform and the remote user ID
    cache: Mutex<HashMap<String, Persona>>,
}

impl Personas {
    /// Loads the personas that `backend` created before.
    pub async fn load(backend: &str, client: Client, database: Database) -> sqlx::Result<Self> {
        let cache = database
            .state(backend)
            .await?
            .into_iter()
            .filter(|(key, _)| key.starts_with(STATE_PREFIX))
            .filter_map(|(key, value)| Some((key, serde_json::from_str(&value).ok()?)))
            .collect();
        Ok(Self {
            backend: backend.to_owned(),
            client,
            database,
            cache: Mutex::new(cache),
        })
    }

    /// Returns a mention like `@dc/vic` for one of the bot's personas.
    pub async fn mention(&self, id: &Id) -> Option<String> {
        let (key, persona) = self
            .cache
            .lock()
            .await
            .iter()
            .find(|(_, persona)| persona.id == id.as_str())
            .map(|(key, persona)| (key.clone(), persona.clone()))?;
        let platform = key.strip_prefix(STATE_PREFIX)?.split_once(':')?.0;
        let source = source_of(platform)?;
        let username = persona.remote_username.as_deref()?;
        Some(format!("@{}/{username}", source.tag()))
    }

    /// Returns the persona of `author`, and creates or updates it to match the author's name and avatar.
    // the lock stays held over the requests, so that two chats do not both create one persona
    #[allow(clippy::significant_drop_tightening)]
    pub async fn persona_for(&self, author: &Author) -> TaskResult<Id> {
        let platform = platform_of(author.source);
        let remote_user_id = author
            .user_id
            .clone()
            .unwrap_or_else(|| author.username.clone());
        let key = format!("{STATE_PREFIX}{platform}:{remote_user_id}");
        let mut cache = self.cache.lock().await;
        let cached = cache.get(&key).cloned();
        let wanted = Persona {
            id: String::new(),
            display_name: author
                .display_name
                .clone()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| author.username.clone()),
            remote_username: Some(author.username.clone()),
            profile_url: profile_url(author),
            // an author without an avatar can be one whose avatar failed to download, so keep the last one
            avatar: avatar_key(author)
                .await
                .or_else(|| cached.as_ref().and_then(|persona| persona.avatar.clone())),
        };

        let current = match cached {
            Some(persona)
                if persona
                    == (Persona {
                        id: persona.id.clone(),
                        ..wanted.clone()
                    }) =>
            {
                return Ok(Id(persona.id));
            }
            Some(persona) => persona,
            None => self.create(platform, &remote_user_id, &wanted).await?,
        };
        let persona = match self.update(&current, &wanted, author).await {
            Err(e) if e.code() == Some("UNKNOWN_IDENTITY") => {
                let created = self.create(platform, &remote_user_id, &wanted).await?;
                self.update(&created, &wanted, author).await?
            }
            result => result?,
        };

        self.database
            .set_state(&self.backend, &key, &serde_json::to_string(&persona)?)
            .await?;
        let id = Id::from(persona.id.as_str());
        cache.insert(key, persona);
        Ok(id)
    }

    async fn create(
        &self,
        platform: &str,
        remote_user_id: &str,
        wanted: &Persona,
    ) -> Result<Persona, kove::Error> {
        let identity = self
            .client
            .create_identity(&CreateIdentity {
                platform: platform.to_owned(),
                remote_user_id: remote_user_id.to_owned(),
                display_name: wanted.display_name.clone(),
                remote_username: wanted.remote_username.clone(),
                profile_url: wanted.profile_url.clone(),
                avatar: None,
            })
            .await?;
        Ok(persona_from(&identity, None))
    }

    async fn update(
        &self,
        current: &Persona,
        wanted: &Persona,
        author: &Author,
    ) -> Result<Persona, kove::Error> {
        fn changed<T: PartialEq + Clone>(current: &T, wanted: &T) -> Option<T> {
            (current != wanted).then(|| wanted.clone())
        }
        let mut update = UpdateIdentity {
            display_name: changed(&current.display_name, &wanted.display_name),
            remote_username: changed(&current.remote_username, &wanted.remote_username),
            profile_url: changed(&current.profile_url, &wanted.profile_url),
            avatar: None,
        };
        let mut avatar = current.avatar.clone();
        if current.avatar != wanted.avatar {
            match self.upload_avatar(author).await {
                Ok(upload) => {
                    update.avatar = Some(upload);
                    avatar.clone_from(&wanted.avatar);
                }
                // the old avatar key stays, so the next message tries again
                Err(e) => warn!(
                    "KoveBackend '{}' failed to upload an avatar of {}: {e}",
                    self.backend, author.username
                ),
            }
        }
        if update.is_empty() {
            return Ok(Persona {
                avatar,
                ..current.clone()
            });
        }
        let identity = self
            .client
            .update_identity(&Id::from(current.id.as_str()), &update)
            .await?;
        Ok(persona_from(&identity, avatar))
    }

    async fn upload_avatar(&self, author: &Author) -> TaskResult<Option<String>> {
        let data = match &author.avatar {
            None => return Ok(None),
            Some(Avatar::Url(url)) => {
                self.client
                    .download(url, UploadPurpose::Avatar.max_size())
                    .await?
            }
            Some(Avatar::File(file)) => Bytes::from(tokio::fs::read(file.file_path()).await?),
        };
        let upload = self
            .client
            .upload(UploadPurpose::Avatar, "avatar", data)
            .await?;
        Ok(Some(upload.id))
    }
}

fn persona_from(identity: &BridgedIdentity, avatar: Option<String>) -> Persona {
    Persona {
        id: identity.id.to_string(),
        display_name: identity.display_name.clone(),
        remote_username: identity.remote_username.clone(),
        profile_url: identity.profile_url.clone(),
        avatar,
    }
}

const PLATFORMS: [(Source, &str); 5] = [
    (Source::File, "file"),
    (Source::Irc, "irc"),
    (Source::Telegram, "telegram"),
    (Source::Discord, "discord"),
    (Source::Kove, "kove"),
];

fn platform_of(source: Source) -> &'static str {
    PLATFORMS
        .iter()
        .find(|(platform_source, _)| *platform_source == source)
        .map_or("unknown", |(_, key)| key)
}

fn source_of(platform: &str) -> Option<Source> {
    PLATFORMS
        .iter()
        .find(|(_, key)| *key == platform)
        .map(|(source, _)| *source)
}

fn profile_url(author: &Author) -> Option<String> {
    match (author.source, &author.user_id) {
        (Source::Discord, Some(id)) => Some(format!("https://discord.com/users/{id}")),
        // a Telegram user without a username has their ID as the username, and no public link
        (Source::Telegram, id) if id.as_ref() != Some(&author.username) => {
            Some(format!("https://t.me/{}", author.username))
        }
        _ => None,
    }
}

async fn avatar_key(author: &Author) -> Option<String> {
    match &author.avatar {
        None => None,
        Some(Avatar::Url(url)) => Some(url.clone()),
        Some(Avatar::File(file)) => tokio::fs::read(file.file_path())
            .await
            .ok()
            .map(|data| sha256::digest(data.as_slice())),
    }
}

#[cfg(test)]
mod tests {
    use super::{platform_of, profile_url, source_of};
    use crate::core::{Author, Source};

    fn author(source: Source, user_id: &str, username: &str) -> Author {
        Author {
            user_id: Some(user_id.to_owned()),
            display_name: None,
            username: username.to_owned(),
            avatar: None,
            source,
        }
    }

    #[test]
    fn maps_platform_keys_both_ways() {
        for source in [Source::Irc, Source::Telegram, Source::Discord] {
            assert_eq!(source_of(platform_of(source)), Some(source));
        }
    }

    #[test]
    fn links_discord_profiles_by_id() {
        assert_eq!(
            profile_url(&author(Source::Discord, "80351110224678912", "vic")).as_deref(),
            Some("https://discord.com/users/80351110224678912")
        );
    }

    #[test]
    fn links_telegram_profiles_by_username() {
        assert_eq!(
            profile_url(&author(Source::Telegram, "42", "vic")).as_deref(),
            Some("https://t.me/vic")
        );
    }

    #[test]
    fn has_no_link_for_telegram_users_without_username() {
        assert_eq!(profile_url(&author(Source::Telegram, "42", "42")), None);
    }
}

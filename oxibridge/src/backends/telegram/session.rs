use std::fmt;

use grammers_session::{
    BoxFuture, Session, SessionData,
    storages::MemorySession,
    types::{DcOption, PeerId, PeerInfo, UpdateState},
};
use serde::{Serialize, de::DeserializeOwned};

use crate::database::Database;

const HOME_DC: &str = "home_dc";
const UPDATES_STATE: &str = "updates_state";
const DC_OPTION_PREFIX: &str = "dc_option:";
const PEER_PREFIX: &str = "peer:";

/// A grammers session that lives in the oxibridge database, as the backend's state.
///
/// Reads come from memory, and every change is also written to the database.
pub struct StoredSession {
    memory: MemorySession,
    database: Database,
    backend: String,
}

#[derive(Debug)]
pub enum SessionError {
    Memory(String),
    Database(sqlx::Error),
    Json(serde_json::Error),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory(e) => write!(f, "session memory: {e}"),
            Self::Database(e) => write!(f, "session database: {e}"),
            Self::Json(e) => write!(f, "session encoding: {e}"),
        }
    }
}

impl std::error::Error for SessionError {}

// a `From` impl for the memory session's error would overlap with `From<T> for T` during coherence checks
fn memory<E: fmt::Display>(e: E) -> SessionError {
    SessionError::Memory(e.to_string())
}

impl From<sqlx::Error> for SessionError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e)
    }
}

impl From<serde_json::Error> for SessionError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl StoredSession {
    /// Loads the session of `backend`. A backend without a stored session starts with an empty one.
    ///
    /// # Errors
    /// Returns an error if the stored session cannot be read or decoded.
    pub async fn load(database: Database, backend: &str) -> Result<Self, SessionError> {
        let mut data = SessionData::default();
        for (key, value) in database.state(backend).await? {
            if key == HOME_DC {
                data.home_dc = decode(&value)?;
            } else if key == UPDATES_STATE {
                data.updates_state = decode(&value)?;
            } else if key.starts_with(DC_OPTION_PREFIX) {
                let dc_option: DcOption = decode(&value)?;
                data.dc_options.insert(dc_option.id, dc_option);
            } else if key.starts_with(PEER_PREFIX) {
                let peer: PeerInfo = decode(&value)?;
                data.peer_infos.insert(peer.id(), peer);
            }
        }

        Ok(Self {
            memory: MemorySession::from(data),
            database,
            backend: backend.to_owned(),
        })
    }

    async fn save<T: Serialize>(&self, key: &str, value: &T) -> Result<(), SessionError> {
        let value = serde_json::to_string(value)?;
        self.database.set_state(&self.backend, key, &value).await?;
        Ok(())
    }
}

fn decode<T: DeserializeOwned>(value: &str) -> Result<T, SessionError> {
    Ok(serde_json::from_str(value)?)
}

fn peer_key(peer: PeerId) -> String {
    format!("{PEER_PREFIX}{peer}")
}

impl Session for StoredSession {
    type Error = SessionError;

    fn home_dc_id(&self) -> Result<i32, SessionError> {
        self.memory.home_dc_id().map_err(memory)
    }

    fn set_home_dc_id(&self, dc_id: i32) -> BoxFuture<'_, Result<(), SessionError>> {
        Box::pin(async move {
            self.memory.set_home_dc_id(dc_id).await.map_err(memory)?;
            self.save(HOME_DC, &dc_id).await
        })
    }

    fn dc_option(&self, dc_id: i32) -> Result<Option<DcOption>, SessionError> {
        self.memory.dc_option(dc_id).map_err(memory)
    }

    fn set_dc_option(&self, dc_option: &DcOption) -> BoxFuture<'_, Result<(), SessionError>> {
        let dc_option = dc_option.clone();
        Box::pin(async move {
            self.memory
                .set_dc_option(&dc_option)
                .await
                .map_err(memory)?;
            self.save(&format!("{DC_OPTION_PREFIX}{}", dc_option.id), &dc_option)
                .await
        })
    }

    fn peer(&self, peer: PeerId) -> BoxFuture<'_, Result<Option<PeerInfo>, SessionError>> {
        Box::pin(async move { self.memory.peer(peer).await.map_err(memory) })
    }

    fn cache_peer(&self, peer: &PeerInfo) -> BoxFuture<'_, Result<(), SessionError>> {
        let peer = peer.clone();
        Box::pin(async move {
            let before = self.memory.peer(peer.id()).await.map_err(memory)?;
            self.memory.cache_peer(&peer).await.map_err(memory)?;
            let after = self.memory.peer(peer.id()).await.map_err(memory)?;
            // grammers caches every peer it sees, so skip the write when nothing is new
            match after {
                Some(after) if before.as_ref() != Some(&after) => {
                    self.save(&peer_key(after.id()), &after).await
                }
                _ => Ok(()),
            }
        })
    }

    fn updates_state(
        &self,
    ) -> BoxFuture<'_, Result<grammers_session::types::UpdatesState, SessionError>> {
        Box::pin(async move { self.memory.updates_state().await.map_err(memory) })
    }

    fn set_update_state(&self, update: UpdateState) -> BoxFuture<'_, Result<(), SessionError>> {
        Box::pin(async move {
            self.memory
                .set_update_state(update)
                .await
                .map_err(memory)?;
            let state = self.memory.updates_state().await.map_err(memory)?;
            self.save(UPDATES_STATE, &state).await
        })
    }
}

#[cfg(test)]
mod tests {
    use grammers_session::{
        Session,
        types::{PeerId, PeerInfo, UpdateState, UpdatesState},
    };

    use super::StoredSession;
    use crate::database::Database;

    #[tokio::test]
    async fn restores_what_was_stored() -> Result<(), Box<dyn std::error::Error>> {
        let database = Database::in_memory().await?;
        let session = StoredSession::load(database.clone(), "tg").await?;
        let peer = PeerInfo::Chat { id: 42 };
        let state = UpdatesState {
            pts: 1,
            qts: 2,
            date: 3,
            seq: 4,
            channels: vec![],
        };
        session.set_home_dc_id(4).await?;
        session.cache_peer(&peer).await?;
        session
            .set_update_state(UpdateState::All(state.clone()))
            .await?;

        let restored = StoredSession::load(database, "tg").await?;
        assert_eq!(restored.home_dc_id()?, 4);
        assert_eq!(
            restored
                .peer(PeerId::chat(42).ok_or("invalid chat ID")?)
                .await?,
            Some(peer)
        );
        assert_eq!(restored.updates_state().await?, state);
        Ok(())
    }

    #[tokio::test]
    async fn keeps_sessions_of_backends_apart() -> Result<(), Box<dyn std::error::Error>> {
        let database = Database::in_memory().await?;
        StoredSession::load(database.clone(), "tg")
            .await?
            .set_home_dc_id(4)
            .await?;
        let other = StoredSession::load(database, "other").await?;
        assert_ne!(other.home_dc_id()?, 4);
        Ok(())
    }
}

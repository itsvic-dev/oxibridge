//! Types of the REST API and the gateway, as described in `/api/v1/openapi.json`.
//!
//! Unknown fields are ignored, and enums have an `Unknown` variant for values that newer servers can send.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A snowflake ID. Kove sends IDs as strings, so they are kept as strings here too.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub String);

impl Id {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Id {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for Id {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    User,
    Bot,
    Bridged,
    Webhook,
    #[serde(other)]
    Unknown,
}

/// The public profile of a user, bot, bridged identity or webhook.
#[derive(Debug, Clone, Deserialize)]
pub struct Actor {
    pub id: Id,
    pub kind: ActorKind,
    /// Users and bots only.
    pub username: Option<String>,
    pub display_name: Option<String>,
    /// A signed URL that expires after a few hours.
    pub avatar_url: Option<String>,
    /// The bot that wears a bridged identity.
    pub bot: Option<BotRef>,
    /// Bridged identities only.
    pub platform: Option<PlatformRef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BotRef {
    pub id: Id,
    pub username: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformRef {
    pub key: String,
    /// False for a key that is not in the registry of `GET /platforms`.
    pub known: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Bot {
    pub id: Id,
    pub username: String,
    pub display_name: Option<String>,
    pub owner_id: Id,
    pub public: bool,
    pub suspended: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Member {
    pub guild_id: Id,
    pub actor: Actor,
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub id: Id,
    pub channel_id: Id,
    pub guild_id: Option<Id>,
    pub author: Actor,
    /// Raw text in the Markdown subset of Kove.
    pub content: String,
    /// The actors that `<@id>` in the content mentions.
    #[serde(default)]
    pub mentions: Vec<Id>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    pub reply_to_id: Option<Id>,
    pub edited_at: Option<String>,
    pub nonce: Option<String>,
    /// The reactions as the viewer sees them. [`None`] in gateway events.
    pub reactions: Option<Vec<Reaction>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Attachment {
    pub id: Id,
    pub filename: String,
    pub content_type: String,
    pub size: u64,
    /// A signed URL that expires after a few hours. [`None`] once the file has expired.
    pub url: Option<String>,
}

/// One emoji on a message, as the viewer sees it.
#[derive(Debug, Clone, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub count: u32,
    /// Whether the viewer reacted with it.
    pub me: bool,
}

/// A remote user that a bot relays as a persona.
#[derive(Debug, Clone, Deserialize)]
pub struct BridgedIdentity {
    pub id: Id,
    pub platform: PlatformRef,
    pub remote_user_id: String,
    pub remote_username: Option<String>,
    pub display_name: String,
    pub profile_url: Option<String>,
    pub avatar_url: Option<String>,
    pub bot: BotRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadPurpose {
    Attachment,
    Avatar,
    Icon,
}

impl UploadPurpose {
    /// The largest file the server takes for this purpose.
    #[must_use]
    pub const fn max_size(self) -> u64 {
        match self {
            Self::Attachment => 25 * 1024 * 1024,
            Self::Avatar | Self::Icon => 8 * 1024 * 1024,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Attachment => "attachment",
            Self::Avatar => "avatar",
            Self::Icon => "icon",
        }
    }
}

impl fmt::Display for UploadPurpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stored upload. Its ID can be used once, for 24 hours, and only by the uploader.
#[derive(Debug, Clone, Deserialize)]
pub struct Upload {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CreateMessage {
    pub content: String,
    /// Upload IDs with the purpose [`UploadPurpose::Attachment`], at most 10.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
    /// A key of at most 64 characters. A retry with the same nonce within 10 minutes returns the first message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<Id>,
    /// Whether a reply also mentions the author of the message it replies to.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reply_mention: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateIdentity {
    /// A key of lowercase letters, digits and `-`, like `discord`.
    pub platform: String,
    pub remote_user_id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_url: Option<String>,
    /// An upload ID with the purpose [`UploadPurpose::Avatar`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

/// A change to a bridged identity. [`None`] keeps a field, and `Some(None)` clears it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateIdentity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_username: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_url: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<Option<String>>,
}

impl UpdateIdentity {
    /// Returns true if the update changes nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.display_name.is_none()
            && self.remote_username.is_none()
            && self.profile_url.is_none()
            && self.avatar.is_none()
    }
}

/// An error body in the Problem Details format (RFC 9457).
#[derive(Debug, Clone, Deserialize)]
pub struct Problem {
    /// A stable code like `UNKNOWN_MESSAGE`. Match on this, not on [`Self::title`].
    pub code: String,
    pub title: String,
    pub status: u16,
    #[serde(default)]
    pub errors: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::{ActorKind, CreateMessage, Id, Message, UpdateIdentity};

    #[test]
    fn reads_a_message_event() -> serde_json::Result<()> {
        let message: Message = serde_json::from_str(
            r#"{
                "id": "1", "channel_id": "2", "guild_id": "3", "content": "hi <@4>",
                "author": { "id": "5", "kind": "user", "username": "vic", "new_field": 1 },
                "mentions": ["4"], "mention_roles": [], "mention_everyone": false,
                "pinned": false, "attachments": []
            }"#,
        )?;
        assert_eq!(message.author.kind, ActorKind::User);
        assert_eq!(message.mentions, vec![Id::from("4")]);
        Ok(())
    }

    #[test]
    fn reads_unknown_actor_kinds() -> serde_json::Result<()> {
        let kind: ActorKind = serde_json::from_str(r#""robot""#)?;
        assert_eq!(kind, ActorKind::Unknown);
        Ok(())
    }

    #[test]
    fn leaves_unset_message_fields_out() -> serde_json::Result<()> {
        let request = CreateMessage {
            content: "hi".to_owned(),
            ..CreateMessage::default()
        };
        assert_eq!(serde_json::to_string(&request)?, r#"{"content":"hi"}"#);
        Ok(())
    }

    #[test]
    fn clears_identity_fields_with_null() -> serde_json::Result<()> {
        let update = UpdateIdentity {
            avatar: Some(None),
            ..UpdateIdentity::default()
        };
        assert_eq!(serde_json::to_string(&update)?, r#"{"avatar":null}"#);
        Ok(())
    }
}

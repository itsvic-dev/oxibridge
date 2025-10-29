use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::types::{Snowflake, User};

#[derive(Debug, Serialize, Deserialize)]
pub struct Webhook {
    /// the id of the webhook
    id: Snowflake,

    /// the type of the webhook
    #[serde(rename = "type")]
    kind: WebhookKind,

    /// the guild id this webhook is for, if any
    #[serde(default)]
    guild_id: Option<Snowflake>,

    /// the channel id this webhook is for, if any
    channel_id: Option<Snowflake>,

    /// the user this webhook was created by (not returned when getting a webhook with its token)
    #[serde(default)]
    user: Option<User>,

    /// the default name of the webhook
    name: Option<String>,

    /// the default user [avatar hash](https://discord.com/developers/docs/reference#image-formatting) of the webhook
    avatar: Option<String>,

    /// the secure token of the webhook (returned for Incoming Webhooks)
    #[serde(default)]
    token: Option<String>,

    /// the bot/OAuth2 application that created this webhook
    application_id: Option<Snowflake>,
}

#[derive(Debug, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum WebhookKind {
    /// Incoming Webhooks can post messages to channels with a generated token
    Incoming = 1,

    /// Channel Follower Webhooks are internal webhooks used with Channel Following to post new messages into channels
    ChannelFollower = 2,

    /// Application webhooks are webhooks used with Interactions
    Application = 3,
}

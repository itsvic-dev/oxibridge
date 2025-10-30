use serde::{Deserialize, Serialize};

use crate::types::Snowflake;

/// See also: [Users Resource](https://discord.com/developers/docs/resources/user)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// the user's id
    pub id: Snowflake,

    /// the user's username, not unique across the platform
    pub username: String,

    /// the user's Discord-tag
    pub discriminator: String,

    /// the user's display name, if it is set. For bots, this is the application name
    pub global_name: Option<String>,

    /// the user's [avatar hash](https://discord.com/developers/docs/reference#image-formatting)
    pub avatar: Option<String>,

    #[serde(default)]
    /// whether the user belongs to an `OAuth2` application
    pub bot: Option<bool>,

    #[serde(default)]
    /// whether the user is an Official Discord System user (part of the urgent message system)
    pub system: Option<bool>,

    #[serde(default)]
    /// the user's [banner hash](https://discord.com/developers/docs/reference#image-formatting)
    pub banner: Option<String>,

    #[serde(default)]
    /// the user's banner color encoded as an integer representation of hexadecimal color code
    pub accent_color: Option<u32>,

    #[serde(default)]
    /// the user's primary guild
    pub primary_guild: Option<UserPrimaryGuild>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPrimaryGuild {
    /// the id of the user's primary guild
    pub identity_guild_id: Option<Snowflake>,

    /// whether the user is displaying the primary guild's server tag. This can be `None` if the system clears the identity, e.g. the server no longer supports tags. This will be `Some(false)` if the user manually removes their tag.
    pub identity_enabled: Option<bool>,

    /// the text of the user's server tag. Limited to 4 characters
    pub tag: Option<String>,

    /// the [server tag badge hash](https://discord.com/developers/docs/reference#image-formatting)
    pub badge: Option<String>,
}

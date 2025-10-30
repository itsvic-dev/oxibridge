use serde_repr::{Deserialize_repr, Serialize_repr};

/// Your connection to Discord's gateway may sometimes close. When it does, you will receive a close code that tells you what happened.
#[derive(Debug, Serialize_repr, Deserialize_repr)]
#[repr(u16)]
pub enum CloseEventCode {
    /// We're not sure what went wrong. Try reconnecting?
    UnknownError = 4000,

    /// You sent an invalid Gateway opcode or an invalid payload for an opcode. Don't do that!
    UnknownOpcode = 4001,

    /// You sent an invalid payload to Discord. Don't do that!
    DecodeError = 4002,

    /// You sent us a payload prior to identifying, or this session has been invalidated.
    NotAuthenticated = 4003,

    /// The account token sent with your identify payload is incorrect.
    AuthenticationFailed = 4004,

    /// You sent more than one identify payload. Don't do that!
    AlreadyAuthenticated = 4005,

    /// The sequence sent when resuming the session was invalid. Reconnect and start a new session.
    InvalidSequence = 4007,

    /// Woah nelly! You're sending payloads to us too quickly. Slow it down! You will be disconnected on receiving this.
    RateLimited = 4008,

    /// Your session timed out. Reconnect and start a new one.
    SessionTimedOut = 4009,

    /// You sent us an invalid shard when identifying.
    InvalidShard = 4010,

    /// The session would have handled too many guilds - you are required to shard your connection in order to connect.
    ShardingRequired = 4011,

    /// You sent an invalid version for the gateway.
    InvalidAPIVersion = 4012,

    /// You sent an invalid intent for a Gateway Intent. You may have incorrectly calculated the bitwise value.
    InvalidIntents = 4013,

    /// You sent a disallowed intent for a Gateway Intent. You may have tried to specify an intent that you have not enabled or are not approved for.
    DisallowedIntents = 4014,
}

impl CloseEventCode {
    /// Returns a boolean that specifies whether you should reconnect upon receiving this error code.
    #[must_use]
    pub const fn reconnect(&self) -> bool {
        !matches!(
            self,
            Self::AuthenticationFailed
                | Self::InvalidShard
                | Self::ShardingRequired
                | Self::InvalidAPIVersion
                | Self::InvalidIntents
                | Self::DisallowedIntents
        )
    }
}

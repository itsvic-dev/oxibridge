use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub shared: SharedConfig,
    pub groups: Vec<GroupConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SharedConfig {
    pub discord_token: String,
    pub telegram_token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GroupConfig {
    pub telegram_chat: i64,
    pub discord_channel: u64,
    pub discord_webhook: String,
}

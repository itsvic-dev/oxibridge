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
    pub r2: Option<R2Config>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct R2Config {
    pub bucket_name: String,
    pub account_id: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GroupConfig {
    pub telegram_chat: i64,
    pub discord_channel: u64,
    pub discord_webhook: String,
}

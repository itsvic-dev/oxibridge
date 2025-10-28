use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub global: GlobalSection,
    #[serde(default)]
    pub backends: HashMap<String, BackendConfig>,
    #[serde(default)]
    pub groups: HashMap<String, HashMap<String, GroupConfig>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GlobalSection {
    pub r2: Option<R2Config>,
    pub cache: Option<CacheConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct R2Config {
    pub bucket_name: String,
    pub account_id: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CacheConfig {
    pub kind: Option<String>, // one of "memory". defaults to "memory"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackendConfig {
    pub kind: String, // one of "discord", "telegram"
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GroupConfig {
    // for discord: guild, channel
    #[serde(default)]
    pub guild: Option<u64>,
    #[serde(default)]
    pub channel: Option<u64>,
    // for telegram: chat
    #[serde(default)]
    pub chat: Option<i64>,

    // shared options
    #[serde(default)]
    pub readonly: bool, // if true, do not send messages to this backend
    #[serde(default)]
    pub writeonly: bool, // if true, do not receive messages from this backend
}

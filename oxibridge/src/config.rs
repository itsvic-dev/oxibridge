use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub global: GlobalSection,
    #[serde(default)]
    pub backends: HashMap<String, BackendConfig>,
    #[serde(default)]
    pub groups: HashMap<String, HashMap<String, GroupBackendConfig>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GlobalSection {
    pub r2: Option<R2Config>,
    #[serde(default)]
    pub cache: CacheConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct R2Config {
    pub bucket_name: String,
    pub account_id: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default)]
    pub kind: CacheKind, // one of "memory". defaults to "memory"
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheKind {
    Memory,
}

impl Default for CacheKind {
    fn default() -> Self {
        Self::Memory
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BackendConfig {
    pub kind: BackendKind, // one of "discord", "telegram"
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Discord,
    Telegram,
    File,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GroupBackendConfig {
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
    /// if true, the backend will not broadcast messages, only receive
    pub readonly: bool,
    #[serde(default)]
    /// if true, the backend will not receive messages, only broadcast
    pub writeonly: bool,
}

use std::collections::HashMap;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackendConfig {
    File(crate::backends::file::Config),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GroupBackendConfig {
    #[serde(default)]
    /// if true, messages are only read from this chat and never written to it
    pub readonly: bool,
    #[serde(default)]
    /// if true, messages are only written to this chat and never read from it
    pub writeonly: bool,

    /// Backend-specific options, parsed by the backend with [`Self::options`].
    #[serde(flatten)]
    pub options: serde_yaml::Mapping,
}

impl GroupBackendConfig {
    /// Parses the backend-specific options of this group.
    ///
    /// # Errors
    /// Returns an error if the options do not match `T`.
    pub fn options<T: DeserializeOwned>(&self) -> Result<T, serde_yaml::Error> {
        serde_yaml::from_value(serde_yaml::Value::Mapping(self.options.clone()))
    }
}

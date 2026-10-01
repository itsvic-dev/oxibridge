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

impl Config {
    /// Checks the parts of the config that serde cannot check.
    ///
    /// # Errors
    /// Returns an error if a group uses a backend that is not defined.
    pub fn validate(&self) -> Result<(), String> {
        for (group_name, group) in &self.groups {
            if let Some(name) = group.keys().find(|name| !self.backends.contains_key(*name)) {
                return Err(format!("group '{group_name}' uses unknown backend '{name}'"));
            }
        }
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn accepts_groups_with_known_backends() -> Result<(), serde_yaml::Error> {
        let config: Config = serde_yaml::from_str(
            "
            backends:
              a: { kind: file, path: a.txt }
            groups:
              g:
                a: { readonly: true }
            ",
        )?;
        assert_eq!(config.validate(), Ok(()));
        Ok(())
    }

    #[test]
    fn rejects_groups_with_unknown_backends() -> Result<(), serde_yaml::Error> {
        let config: Config = serde_yaml::from_str(
            "
            backends:
              a: { kind: file, path: a.txt }
            groups:
              g:
                b: {}
            ",
        )?;
        assert!(config.validate().is_err());
        Ok(())
    }
}

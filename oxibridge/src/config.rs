use std::{collections::HashMap, path::PathBuf};

use color_eyre::{Section, eyre::WrapErr};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_yaml::Value;

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
    /// Reads the YAML files in `paths` and deep-merges them in order.
    /// Values from later files override values from earlier ones.
    ///
    /// # Errors
    /// Returns an error if a file cannot be read or the merged config is invalid.
    pub async fn load(paths: &[&str]) -> color_eyre::Result<Self> {
        let mut merged = Value::Null;
        for path in paths {
            let file = tokio::fs::read(path)
                .await
                .wrap_err_with(|| format!("failed to read config file '{path}'"))
                .suggestion("Create a `config.yml` file and fill it out. Look at `config.example.yml` for reference.")?;
            let value = serde_yaml::from_slice(&file)
                .wrap_err_with(|| format!("failed to parse config file '{path}'"))?;
            merge(&mut merged, value);
        }
        Ok(serde_yaml::from_value(merged)?)
    }

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

fn merge(base: &mut Value, other: Value) {
    match (base, other) {
        (_, Value::Null) => {}
        (Value::Mapping(base), Value::Mapping(other)) => {
            for (key, value) in other {
                match base.get_mut(&key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, other) => *base = other,
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GlobalSection {
    pub r2: Option<R2Config>,
    #[serde(default)]
    pub cache: CacheConfig,
    /// Path of the SQLite database. Created if it does not exist.
    #[serde(default = "default_database")]
    pub database: PathBuf,
}

impl Default for GlobalSection {
    fn default() -> Self {
        Self {
            r2: None,
            cache: CacheConfig::default(),
            database: default_database(),
        }
    }
}

fn default_database() -> PathBuf {
    PathBuf::from("oxibridge.db")
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
    Irc(crate::backends::irc::Config),
    Telegram(crate::backends::telegram::Config),
    Discord(crate::backends::discord::Config),
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
    use super::{Config, merge};
    use serde_yaml::Value;

    fn merged(files: &[&str]) -> Result<Value, serde_yaml::Error> {
        let mut base = Value::Null;
        for file in files {
            merge(&mut base, serde_yaml::from_str(file)?);
        }
        Ok(base)
    }

    #[test]
    fn merges_nested_mappings_from_both_files() -> Result<(), serde_yaml::Error> {
        let result = merged(&["a: { x: 1 }", "a: { y: 2 }"])?;
        assert_eq!(result, serde_yaml::from_str::<Value>("a: { x: 1, y: 2 }")?);
        Ok(())
    }

    #[test]
    fn later_files_override_earlier_values() -> Result<(), serde_yaml::Error> {
        let result = merged(&["a: { x: 1, list: [1, 2] }", "a: { x: 2, list: [3] }"])?;
        assert_eq!(result, serde_yaml::from_str::<Value>("a: { x: 2, list: [3] }")?);
        Ok(())
    }

    #[test]
    fn empty_files_change_nothing() -> Result<(), serde_yaml::Error> {
        let result = merged(&["a: 1", ""])?;
        assert_eq!(result, serde_yaml::from_str::<Value>("a: 1")?);
        Ok(())
    }

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

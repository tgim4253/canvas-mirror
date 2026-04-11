use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    /// Socket address the server listens on.
    #[serde(default = "default_bind_addr")]
    pub bind_addr: SocketAddr,
    /// Optional external base URL clients should use to reach the server.
    /// When omitted, wrappers may derive candidate local/LAN URLs from `bind_addr`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_url: Option<Url>,
    /// Milliseconds before room/device state is treated as stale.
    #[serde(default = "default_stale_timeout_ms")]
    pub stale_timeout_ms: u64,
    /// Path to the persisted room store file loaded at startup.
    #[serde(default = "default_store_path")]
    pub store_path: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            bind_addr: default_bind_addr(),
            public_url: None,
            stale_timeout_ms: default_stale_timeout_ms(),
            store_path: default_store_path(),
        }
    }
}

impl ServerConfig {
    pub fn from_json_str(input: &str) -> Result<Self, ConfigError> {
        serde_json::from_str(input).map_err(ConfigError::JsonDe)
    }

    pub fn to_json_string(&self) -> Result<String, ConfigError> {
        serde_json::to_string_pretty(self).map_err(ConfigError::JsonSer)
    }

    pub fn from_toml_str(input: &str) -> Result<Self, ConfigError> {
        toml::from_str(input).map_err(ConfigError::TomlDe)
    }

    pub fn to_toml_string(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(ConfigError::TomlSer)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)?;
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Self::from_json_str(&content),
            _ => Self::from_toml_str(&content),
        }
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let path = path.as_ref();
        let content = match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => self.to_json_string()?,
            _ => self.to_toml_string()?,
        };
        fs::write(path, content)?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse json config: {0}")]
    JsonDe(serde_json::Error),
    #[error("failed to encode json config: {0}")]
    JsonSer(serde_json::Error),
    #[error("failed to parse toml config: {0}")]
    TomlDe(toml::de::Error),
    #[error("failed to encode toml config: {0}")]
    TomlSer(toml::ser::Error),
}

fn default_bind_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8787))
}

fn default_stale_timeout_ms() -> u64 {
    30_000
}

fn default_store_path() -> PathBuf {
    PathBuf::from("./image-server-store.toml")
}

fn default_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn default_config_has_expected_defaults() {
        let config = ServerConfig::default();

        assert_eq!(config.version, 1);
        assert_eq!(config.bind_addr, SocketAddr::from(([127, 0, 0, 1], 8787)));
        assert_eq!(config.public_url, None);
        assert_eq!(config.stale_timeout_ms, 30_000);
        assert_eq!(
            config.store_path,
            PathBuf::from("./image-server-store.toml")
        );
    }

    #[test]
    fn toml_round_trip_preserves_server_options() {
        let config = sample_config();

        let toml = config.to_toml_string().expect("toml encoding should work");
        let decoded = ServerConfig::from_toml_str(&toml).expect("toml decoding should work");

        assert_eq!(decoded, config);
    }

    #[test]
    fn json_round_trip_preserves_server_options() {
        let config = sample_config();

        let json = config.to_json_string().expect("json encoding should work");
        let decoded = ServerConfig::from_json_str(&json).expect("json decoding should work");

        assert_eq!(decoded, config);
    }

    #[test]
    fn save_and_load_json_file_round_trip() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let path = dir.path().join("server-config.json");
        let config = sample_config();

        config
            .save_to_path(&path)
            .expect("config file should be saved");
        let loaded = ServerConfig::load_from_path(&path).expect("config file should be loaded");

        let raw = fs::read_to_string(path).expect("saved file should be readable");
        assert!(raw.contains("\"bind_addr\""));
        assert!(raw.contains("\"public_url\""));
        assert!(raw.contains("\"store_path\""));
        assert_eq!(loaded, config);
    }

    #[test]
    fn save_and_load_toml_file_round_trip() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let path = dir.path().join("server-config.toml");
        let config = sample_config();

        config
            .save_to_path(&path)
            .expect("config file should be saved");
        let loaded = ServerConfig::load_from_path(&path).expect("config file should be loaded");

        let raw = fs::read_to_string(path).expect("saved file should be readable");
        assert!(raw.contains("store_path = \"./config/rooms.toml\""));
        assert_eq!(loaded, config);
    }

    #[test]
    fn invalid_bind_addr_returns_toml_decode_error() {
        let error = ServerConfig::from_toml_str(
            r#"
                bind_addr = "not-an-addr"
                public_url = "http://127.0.0.1:8787"
            "#,
        )
        .expect_err("invalid bind address should fail");

        assert!(matches!(error, ConfigError::TomlDe(_)));
    }

    #[test]
    fn invalid_public_url_returns_json_decode_error() {
        let error = ServerConfig::from_json_str(
            r#"{
                "bind_addr":"127.0.0.1:8787",
                "public_url":"not a url",
                "store_path":"./image-server-store.toml"
            }"#,
        )
        .expect_err("invalid URL should fail");

        assert!(matches!(error, ConfigError::JsonDe(_)));
    }

    #[test]
    fn invalid_toml_shape_returns_toml_decode_error() {
        let error = ServerConfig::from_toml_str("store_path = 42")
            .expect_err("invalid store path should fail");

        assert!(matches!(error, ConfigError::TomlDe(_)));
    }

    fn sample_config() -> ServerConfig {
        ServerConfig {
            version: 1,
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 8787)),
            public_url: Some(
                "http://127.0.0.1:8787"
                    .parse()
                    .expect("sample URL must be valid"),
            ),
            stale_timeout_ms: 30_000,
            store_path: PathBuf::from("./config/rooms.toml"),
        }
    }
}

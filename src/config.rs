use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Persistent configuration, stored as YAML at
/// `~/.config/odysseus-code/config.yaml` (or `$ODYSSEUS_CODE_CONFIG_DIR/config.yaml`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    /// Base URL of the Odysseus instance.
    pub endpoint: String,
    /// Odysseus API token (`ody_…`), created in Settings → Integrations →
    /// API Tokens (admin only).
    pub api_key: String,
    /// Preferred model ID for new sessions (empty = first available).
    pub model: String,
    /// Odysseus model-endpoint ID used when creating sessions (empty = resolve
    /// from /api/models).
    pub endpoint_id: String,
    /// Language assumed when none can be inferred from the current file.
    pub default_language: String,
    /// Container image used by the `run` sandbox.
    pub sandbox_image: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:7000".into(),
            api_key: String::new(),
            model: String::new(),
            endpoint_id: String::new(),
            default_language: "rust".into(),
            sandbox_image: "rust:slim".into(),
        }
    }
}

const KEYS: &[&str] = &[
    "endpoint",
    "api_key",
    "model",
    "endpoint_id",
    "default_language",
    "sandbox_image",
];

impl Config {
    /// Load the config file, creating it with defaults on first run, then
    /// apply `ODYSSEUS_URL` / `ODYSSEUS_API_TOKEN` env overrides (same
    /// convention as the Odysseus integration scripts). Env values are never
    /// written back to disk.
    pub fn load() -> Result<Self> {
        let mut cfg = Self::load_file(&config_path()?)?;
        if let Ok(url) = std::env::var("ODYSSEUS_URL")
            && !url.trim().is_empty()
        {
            cfg.endpoint = url.trim().trim_end_matches('/').to_string();
        }
        if let Ok(token) = std::env::var("ODYSSEUS_API_TOKEN")
            && !token.trim().is_empty()
        {
            cfg.api_key = token.trim().to_string();
        }
        Ok(cfg)
    }

    /// Load exactly what is on disk (no env overrides). Used by `config set`
    /// so environment values are not accidentally persisted.
    pub fn load_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            let cfg = Self::default();
            cfg.save_to(path)?;
            return Ok(cfg);
        }
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file {}", path.display()))?;
        serde_yaml::from_str(&raw)
            .with_context(|| format!("parsing config file {}", path.display()))
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating config directory {}", dir.display()))?;
        }
        let yaml = serde_yaml::to_string(self)?;
        std::fs::write(path, yaml)
            .with_context(|| format!("writing config file {}", path.display()))
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "endpoint" => self.endpoint = value.trim_end_matches('/').to_string(),
            "api_key" => self.api_key = value.to_string(),
            "model" => self.model = value.to_string(),
            "endpoint_id" => self.endpoint_id = value.to_string(),
            "default_language" => self.default_language = value.to_lowercase(),
            "sandbox_image" => self.sandbox_image = value.to_string(),
            other => bail!(
                "unknown config key '{other}' (valid keys: {})",
                KEYS.join(", ")
            ),
        }
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<String> {
        Ok(match key {
            "endpoint" => self.endpoint.clone(),
            "api_key" => self.api_key.clone(),
            "model" => self.model.clone(),
            "endpoint_id" => self.endpoint_id.clone(),
            "default_language" => self.default_language.clone(),
            "sandbox_image" => self.sandbox_image.clone(),
            other => bail!(
                "unknown config key '{other}' (valid keys: {})",
                KEYS.join(", ")
            ),
        })
    }
}

pub fn config_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("ODYSSEUS_CODE_CONFIG_DIR")
        && !dir.trim().is_empty()
    {
        return Ok(PathBuf::from(dir));
    }
    Ok(dirs::config_dir()
        .context("could not determine the user config directory")?
        .join("odysseus-code"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.yaml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_load_writes_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let cfg = Config::load_file(&path).unwrap();
        assert_eq!(cfg, Config::default());
        assert!(path.exists(), "defaults should be persisted on first load");
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("endpoint: http://localhost:7000"));
        assert!(raw.contains("default_language: rust"));
    }

    #[test]
    fn set_persists_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let mut cfg = Config::load_file(&path).unwrap();
        cfg.set("endpoint", "http://example.com:9999/").unwrap();
        cfg.set("api_key", "ody_test123").unwrap();
        cfg.save_to(&path).unwrap();

        let reloaded = Config::load_file(&path).unwrap();
        // trailing slash is normalized away
        assert_eq!(reloaded.endpoint, "http://example.com:9999");
        assert_eq!(reloaded.api_key, "ody_test123");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let mut cfg = Config::default();
        assert!(cfg.set("nope", "x").is_err());
        assert!(cfg.get("nope").is_err());
    }

    #[test]
    fn get_returns_each_key() {
        let cfg = Config::default();
        for key in KEYS {
            cfg.get(key).unwrap();
        }
    }
}

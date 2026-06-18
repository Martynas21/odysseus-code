use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Persistent configuration, stored as YAML at
/// `~/.config/odysseus-code/config.yaml` (or `$ODYSSEUS_CODE_CONFIG_DIR/config.yaml`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Base URL of the OpenAI-compatible server (no `/v1` suffix).
    #[serde(alias = "endpoint")]
    pub base_url: String,
    /// Optional bearer token. Empty = send no Authorization header (local
    /// servers usually need none).
    pub api_key: String,
    /// Model id to request.
    pub model: String,
    /// Sampling temperature.
    pub temperature: f32,
    /// Max tokens to generate per turn.
    pub max_tokens: u32,
    /// Per-tool execution timeout (seconds).
    pub tool_timeout_secs: u64,
    /// "prompt" (gate mutating tools), "auto" (run all), or "readonly"
    /// (auto-run read-only, auto-deny mutating).
    pub approval_policy: String,
    /// Language assumed when none can be inferred from the current file.
    pub default_language: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:1234".into(),
            api_key: String::new(),
            model: String::new(),
            temperature: 0.2,
            max_tokens: 32768,
            tool_timeout_secs: 60,
            approval_policy: "prompt".into(),
            default_language: "rust".into(),
        }
    }
}

impl Config {
    fn keys() -> Vec<String> {
        let value = serde_yaml::to_value(Config::default()).expect("Config serializes");
        value
            .as_mapping()
            .map(|m| {
                m.keys()
                    .filter_map(|k| k.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Load the config file, creating it with defaults on first run, then
    /// apply `ODYSSEUS_URL` / `ODYSSEUS_API_TOKEN` env overrides (same
    /// convention as the Odysseus integration scripts). Env values are never
    /// written back to disk.
    pub fn load() -> Result<Self> {
        let mut cfg = Self::load_file(&config_path()?)?;
        for var in ["ODYSSEUS_BASE_URL", "ODYSSEUS_URL"] {
            if let Ok(v) = std::env::var(var)
                && !v.trim().is_empty()
            {
                cfg.base_url = v.trim().trim_end_matches('/').to_string();
            }
        }
        for var in ["ODYSSEUS_API_KEY", "ODYSSEUS_API_TOKEN"] {
            if let Ok(v) = std::env::var(var)
                && !v.trim().is_empty()
            {
                cfg.api_key = v.trim().to_string();
            }
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
            // The config holds an `ody_…` token, so keep the directory private.
            restrict_permissions(dir, 0o700)?;
        }
        let yaml = serde_yaml::to_string(self)?;
        // Create the file already private (0600 on unix) so the secret API
        // token is never momentarily world-readable between write and chmod.
        write_file_private(path, &yaml)?;
        Ok(())
    }

    pub fn apply_overrides(&mut self, model: Option<&str>, base_url: Option<&str>) {
        if let Some(m) = model {
            self.model = m.to_string();
        }
        if let Some(b) = base_url {
            self.base_url = b.trim_end_matches('/').to_string();
        }
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "base_url" => self.base_url = value.trim_end_matches('/').to_string(),
            "api_key" => self.api_key = value.to_string(),
            "model" => self.model = value.to_string(),
            "temperature" => {
                self.temperature = value.parse().context("temperature must be a number")?
            }
            "max_tokens" => {
                self.max_tokens = value.parse().context("max_tokens must be an integer")?
            }
            "tool_timeout_secs" => {
                self.tool_timeout_secs = value
                    .parse()
                    .context("tool_timeout_secs must be an integer")?
            }
            "approval_policy" => match value {
                "prompt" | "auto" | "readonly" => self.approval_policy = value.to_string(),
                other => bail!("approval_policy must be prompt|auto|readonly, got '{other}'"),
            },
            "default_language" => self.default_language = value.to_lowercase(),
            other => bail!(
                "unknown config key '{other}' (valid keys: {})",
                Self::keys().join(", ")
            ),
        }
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<String> {
        let value = serde_yaml::to_value(self).expect("Config serializes");
        let mapping = value.as_mapping().expect("Config serializes to a mapping");
        let scalar = mapping
            .get(serde_yaml::Value::from(key))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown config key '{key}' (valid keys: {})",
                    Self::keys().join(", ")
                )
            })?;
        Ok(match scalar {
            serde_yaml::Value::String(s) => s.clone(),
            serde_yaml::Value::Number(n) => n.to_string(),
            serde_yaml::Value::Bool(b) => b.to_string(),
            serde_yaml::Value::Null => String::new(),
            other => serde_yaml::to_string(other)?.trim().to_string(),
        })
    }
}

/// Write `contents` to `path`, creating the file private to the owner (0600 on
/// unix) so the API token is never momentarily world-readable. An existing file
/// keeps its old mode through `OpenOptions`, so it is re-secured afterward too.
fn write_file_private(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(path)
        .with_context(|| format!("writing config file {}", path.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("writing config file {}", path.display()))?;
    restrict_permissions(path, 0o600)?;
    Ok(())
}

/// Restrict a config path (dir or file) to the owner. No-op on non-unix.
fn restrict_permissions(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .with_context(|| format!("securing {}", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
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
        assert!(raw.contains("base_url: http://localhost:1234"));
        assert!(raw.contains("default_language: rust"));
    }

    #[test]
    fn set_persists_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let mut cfg = Config::load_file(&path).unwrap();
        cfg.set("base_url", "http://example.com:9999/").unwrap();
        cfg.set("api_key", "ody_test123").unwrap();
        cfg.save_to(&path).unwrap();

        let reloaded = Config::load_file(&path).unwrap();
        // trailing slash is normalized away
        assert_eq!(reloaded.base_url, "http://example.com:9999");
        assert_eq!(reloaded.api_key, "ody_test123");
    }

    #[test]
    fn endpoint_alias_migrates_to_base_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "endpoint: http://old:7000\napi_key: ody_x\n").unwrap();
        let cfg = Config::load_file(&path).unwrap();
        assert_eq!(cfg.base_url, "http://old:7000");
    }

    #[test]
    fn unknown_legacy_keys_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, "base_url: http://x\nendpoint_id: ep1\n").unwrap();
        let cfg = Config::load_file(&path).unwrap();
        assert_eq!(cfg.base_url, "http://x");
    }

    #[test]
    fn agent_defaults_are_sane() {
        let cfg = Config::default();
        assert_eq!(cfg.base_url, "http://localhost:1234");
        assert_eq!(cfg.temperature, 0.2);
        assert_eq!(cfg.max_tokens, 32768);
        assert_eq!(cfg.tool_timeout_secs, 60);
        assert_eq!(cfg.approval_policy, "prompt");
    }

    #[test]
    fn set_and_get_new_keys() {
        let mut cfg = Config::default();
        cfg.set("base_url", "http://h:1/").unwrap();
        cfg.set("approval_policy", "auto").unwrap();
        assert_eq!(cfg.get("base_url").unwrap(), "http://h:1"); // trailing slash trimmed
        assert_eq!(cfg.get("approval_policy").unwrap(), "auto");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let mut cfg = Config::default();
        assert!(cfg.set("nope", "x").is_err());
        assert!(cfg.get("nope").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn save_to_restricts_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        Config::default().save_to(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn get_returns_each_key() {
        let cfg = Config::default();
        for key in Config::keys() {
            cfg.get(&key).unwrap();
        }
    }

    #[test]
    fn every_key_round_trips_through_set_and_get() {
        let cases = [
            ("base_url", "http://h:1"),
            ("api_key", "k"),
            ("model", "m"),
            ("temperature", "0.5"),
            ("max_tokens", "100"),
            ("tool_timeout_secs", "30"),
            ("approval_policy", "auto"),
            ("default_language", "go"),
        ];
        for key in Config::keys() {
            assert!(
                cases.iter().any(|(c, _)| *c == key),
                "no set case for config key '{key}'"
            );
        }
        for (key, value) in cases {
            let mut cfg = Config::default();
            cfg.set(key, value).unwrap();
            cfg.get(key).unwrap();
        }
    }
}

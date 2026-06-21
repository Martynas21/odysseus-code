use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    #[serde(alias = "endpoint")]
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub tool_timeout_secs: u64,
    pub approval_policy: String,
    pub default_language: String,
    pub searxng_url: String,
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
            searxng_url: String::new(),
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
        if let Ok(v) = std::env::var("ODYSSEUS_SEARXNG_URL")
            && !v.trim().is_empty()
        {
            cfg.searxng_url = v.trim().trim_end_matches('/').to_string();
        }
        Ok(cfg)
    }

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
            restrict_permissions(dir, 0o700)?;
        }
        let yaml = serde_yaml::to_string(self)?;
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
            "searxng_url" => self.searxng_url = value.trim_end_matches('/').to_string(),
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
        let scalar = mapping.get(serde_yaml::Value::from(key)).ok_or_else(|| {
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
mod tests;

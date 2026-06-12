use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Name of the server session that sessionless prompts lazily create/reuse.
pub const DEFAULT_SESSION_NAME: &str = "odysseus-code";

/// Local map of friendly session names to Odysseus server session IDs.
/// Conversation history itself lives server-side; this store only remembers
/// which server session a name refers to and which one is active.
/// Persisted at `~/.cache/odysseus-code/sessions.json`
/// (or `$ODYSSEUS_CODE_CACHE_DIR/sessions.json`).
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionStore {
    #[serde(default)]
    sessions: BTreeMap<String, String>,
    #[serde(default)]
    active: Option<String>,
}

impl SessionStore {
    pub fn load() -> Result<Self> {
        Self::load_from(&store_path()?)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading session store {}", path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing session store {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&store_path()?)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating cache directory {}", dir.display()))?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("writing session store {}", path.display()))
    }

    /// Server session ID for a friendly name.
    pub fn server_id(&self, name: &str) -> Option<&str> {
        self.sessions.get(name).map(String::as_str)
    }

    pub fn insert(&mut self, name: &str, server_id: &str) {
        self.sessions
            .insert(name.to_string(), server_id.to_string());
    }

    /// Remove a mapping; clears the active pointer if it referred to it.
    /// Returns the removed server ID.
    pub fn remove(&mut self, name: &str) -> Option<String> {
        if self.active.as_deref() == Some(name) {
            self.active = None;
        }
        self.sessions.remove(name)
    }

    /// The active session as (name, server_id), if any.
    pub fn active(&self) -> Option<(&str, &str)> {
        let name = self.active.as_deref()?;
        Some((name, self.server_id(name)?))
    }

    pub fn set_active(&mut self, name: &str) {
        self.active = Some(name.to_string());
    }
}

pub fn store_path() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("ODYSSEUS_CODE_CACHE_DIR")
        && !dir.trim().is_empty()
    {
        return Ok(PathBuf::from(dir).join("sessions.json"));
    }
    Ok(dirs::cache_dir()
        .context("could not determine the user cache directory")?
        .join("odysseus-code")
        .join("sessions.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_loads_empty_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::load_from(&dir.path().join("sessions.json")).unwrap();
        assert_eq!(store, SessionStore::default());
    }

    #[test]
    fn roundtrip_persists_sessions_and_active() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.json");

        let mut store = SessionStore::default();
        store.insert("my-project", "srv-123");
        store.set_active("my-project");
        store.save_to(&path).unwrap();

        let loaded = SessionStore::load_from(&path).unwrap();
        assert_eq!(loaded.server_id("my-project"), Some("srv-123"));
        assert_eq!(loaded.active(), Some(("my-project", "srv-123")));
    }

    #[test]
    fn remove_clears_active_pointer() {
        let mut store = SessionStore::default();
        store.insert("a", "1");
        store.set_active("a");
        assert_eq!(store.remove("a"), Some("1".to_string()));
        assert_eq!(store.active(), None);
    }

    #[test]
    fn active_with_dangling_name_is_none() {
        let mut store = SessionStore::default();
        store.set_active("ghost");
        assert_eq!(store.active(), None);
    }
}

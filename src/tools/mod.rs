pub mod ask_user;
pub mod fs_read;
pub mod fs_write;
pub mod search;
pub mod shell;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

use crate::llm::ToolDef;
use crate::mode::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Safety {
    ReadOnly,
    Mutating,
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("bad arguments: {0}")]
    BadArgs(String),
    #[error("path escapes the workspace: {0}")]
    PathEscape(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("{0}")]
    Failed(String),
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn parameters(&self) -> Value;
    fn safety(&self) -> Safety;

    /// Whether the agent loop must handle this tool by blocking for user input
    /// (e.g. an interactive question) instead of calling `execute`.
    fn interactive(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        args: &Value,
        cwd: &Path,
        timeout_secs: u64,
    ) -> Result<String, ToolError>;

    fn def(&self) -> ToolDef {
        ToolDef {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
        }
    }
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn default_set() -> Self {
        Self {
            tools: vec![
                Box::new(fs_read::ReadFile),
                Box::new(fs_read::ListDir),
                Box::new(fs_write::WriteFile { spec_only: false }),
                Box::new(fs_write::EditFile),
                Box::new(search::Grep),
                Box::new(shell::Shell),
                Box::new(ask_user::AskUser),
            ],
        }
    }

    /// Build the toolset appropriate for the given mode. Spec mode gets the
    /// read-only tools plus a spec-restricted writer (no `edit_file`, no
    /// `shell`), so the agent cannot modify source code.
    pub fn for_mode(mode: Mode) -> Self {
        match mode {
            Mode::Implement => Self::default_set(),
            Mode::Spec => Self {
                tools: vec![
                    Box::new(fs_read::ReadFile),
                    Box::new(fs_read::ListDir),
                    Box::new(search::Grep),
                    Box::new(fs_write::WriteFile { spec_only: true }),
                    Box::new(ask_user::AskUser),
                ],
            },
        }
    }

    pub fn defs(&self) -> Vec<ToolDef> {
        self.tools.iter().map(|t| t.def()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|b| b.as_ref())
    }
}

pub fn resolve_in_repo(cwd: &Path, rel: &str) -> Result<PathBuf, ToolError> {
    let root = cwd
        .canonicalize()
        .map_err(|e| ToolError::Io(format!("workspace {}: {e}", cwd.display())))?;
    let joined = root.join(rel);
    let check = if joined.exists() {
        joined
            .canonicalize()
            .map_err(|e| ToolError::Io(e.to_string()))?
    } else {
        let mut ancestor = joined.as_path();
        let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
        let real = loop {
            match ancestor.canonicalize() {
                Ok(real) => break real,
                Err(_) => {
                    let name = ancestor
                        .file_name()
                        .ok_or_else(|| ToolError::PathEscape(rel.to_string()))?;
                    tail.push(name);
                    ancestor = ancestor
                        .parent()
                        .ok_or_else(|| ToolError::PathEscape(rel.to_string()))?;
                }
            }
        };
        let mut check = real;
        for name in tail.into_iter().rev() {
            check.push(name);
        }
        check
    };
    if !check.starts_with(&root) {
        return Err(ToolError::PathEscape(rel.to_string()));
    }
    Ok(check)
}

pub(crate) fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::BadArgs(format!("missing string '{key}'")))
}

pub(crate) fn truncate(s: String, max: usize) -> String {
    if s.len() <= max {
        s
    } else {
        let cut = s
            .char_indices()
            .take_while(|(i, _)| *i < max)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(max);
        format!("{}\n…[truncated]", &s[..cut])
    }
}

/// Truncate to at most `max` characters, appending `suffix` when shortened.
/// Char-based (not byte-based), so it never splits a multi-byte character.
pub(crate) fn truncate_chars(s: &str, max: usize, suffix: &str) -> String {
    if s.chars().count() > max {
        format!("{}{suffix}", s.chars().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_in_repo_accepts_paths_inside() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "x").unwrap();
        let p = resolve_in_repo(dir.path(), "a.txt").unwrap();
        assert!(p.ends_with("a.txt"));
    }

    #[test]
    fn resolve_in_repo_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_in_repo(dir.path(), "../escape.txt");
        assert!(err.is_err());
    }

    #[test]
    fn registry_exposes_defs_and_safety() {
        let reg = ToolRegistry::default_set();
        let defs = reg.defs();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"shell"));
        assert_eq!(
            reg.get("read_file").map(|t| t.safety()),
            Some(Safety::ReadOnly)
        );
        assert_eq!(reg.get("shell").map(|t| t.safety()), Some(Safety::Mutating));
        assert!(reg.get("nope").is_none());
    }

    #[test]
    fn spec_mode_excludes_code_mutating_tools() {
        let names: Vec<String> = ToolRegistry::for_mode(Mode::Spec)
            .defs()
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert!(names.iter().any(|n| n == "read_file"));
        assert!(names.iter().any(|n| n == "grep"));
        assert!(names.iter().any(|n| n == "write_file"));
        assert!(!names.iter().any(|n| n == "edit_file"));
        assert!(!names.iter().any(|n| n == "shell"));
        assert!(names.iter().any(|n| n == "ask_user"));
    }

    #[test]
    fn implement_mode_matches_default_set() {
        let implement: Vec<String> = ToolRegistry::for_mode(Mode::Implement)
            .defs()
            .into_iter()
            .map(|d| d.name)
            .collect();
        let default: Vec<String> = ToolRegistry::default_set()
            .defs()
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert_eq!(implement, default);
    }
}

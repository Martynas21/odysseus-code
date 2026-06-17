pub mod fs_read;
pub mod fs_write;
pub mod search;
pub mod shell;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

use crate::llm::ToolDef;

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
                Box::new(fs_write::WriteFile),
                Box::new(fs_write::EditFile),
                Box::new(search::Grep),
                Box::new(shell::Shell),
            ],
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

    pub fn safety(&self, name: &str) -> Option<Safety> {
        self.get(name).map(|t| t.safety())
    }
}

/// Resolve `rel` (a tool-supplied path) against the workspace `cwd`, then
/// canonicalize and verify it stays inside the workspace. The parent of a
/// not-yet-existing file is checked instead, so writes to new files are allowed.
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
        // The target does not exist yet (a write to a new file, possibly under
        // not-yet-created parent dirs). Canonicalize the nearest existing
        // ancestor, then re-append the trailing components so containment can
        // still be checked. Reject any path that resolves outside `root`.
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
    // `root` is already canonical, so it is the containment prefix directly.
    if !check.starts_with(&root) {
        return Err(ToolError::PathEscape(rel.to_string()));
    }
    Ok(check)
}

/// Truncate tool output so a runaway command can't flood the model context.
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
        assert_eq!(reg.safety("read_file"), Some(Safety::ReadOnly));
        assert_eq!(reg.safety("shell"), Some(Safety::Mutating));
        assert_eq!(reg.safety("nope"), None);
    }
}

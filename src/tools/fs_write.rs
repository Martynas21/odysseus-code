use std::path::Path;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::mode::SPEC_DIR;

use super::{Safety, Tool, ToolError, resolve_in_repo, str_arg};

/// Writes a file in the workspace. When `spec_only` is set (spec mode) writes
/// are restricted to `*.md` files under `docs/edds/`, so the agent cannot edit
/// source code while building a specification.
pub struct WriteFile {
    pub spec_only: bool,
}

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &'static str {
        "write_file"
    }
    fn description(&self) -> &'static str {
        if self.spec_only {
            "Create or overwrite a specification document (a *.md file under docs/edds/). \
             In spec mode these are the only files you may write."
        } else {
            "Create or overwrite a file in the workspace with the given content."
        }
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"]
        })
    }
    fn safety(&self) -> Safety {
        Safety::Mutating
    }
    async fn execute(&self, args: &Value, cwd: &Path, _t: u64) -> Result<String, ToolError> {
        let raw = str_arg(args, "path")?;
        let path = resolve_in_repo(cwd, raw)?;
        if self.spec_only {
            let spec_dir = resolve_in_repo(cwd, SPEC_DIR)?;
            let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");
            if !path.starts_with(&spec_dir) || !is_md {
                return Err(ToolError::Failed(format!(
                    "spec mode can only write *.md files under {SPEC_DIR}/; \
                     switch to implement mode (Shift+Tab) to change code",
                )));
            }
        }
        let content = str_arg(args, "content")?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::Io(e.to_string()))?;
        }
        tokio::fs::write(&path, content)
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?;
        Ok(format!("wrote {} bytes to {raw}", content.len()))
    }
}

pub struct EditFile;

#[async_trait]
impl Tool for EditFile {
    fn name(&self) -> &'static str {
        "edit_file"
    }
    fn description(&self) -> &'static str {
        "Replace a unique occurrence of `old` with `new` in a file. Set replace_all to replace every occurrence."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old": {"type": "string"},
                "new": {"type": "string"},
                "replace_all": {"type": "boolean"}
            },
            "required": ["path", "old", "new"]
        })
    }
    fn safety(&self) -> Safety {
        Safety::Mutating
    }
    async fn execute(&self, args: &Value, cwd: &Path, _t: u64) -> Result<String, ToolError> {
        let raw = str_arg(args, "path")?;
        let path = resolve_in_repo(cwd, raw)?;
        let old = str_arg(args, "old")?;
        let new = str_arg(args, "new")?;
        let replace_all = args
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let text = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?;
        let count = text.matches(old).count();
        if count == 0 {
            return Err(ToolError::Failed(format!(
                "`old` not found in {}",
                path.display()
            )));
        }
        if count > 1 && !replace_all {
            return Err(ToolError::Failed(format!(
                "`old` occurs {count} times in {}; pass replace_all or a more specific match",
                path.display()
            )));
        }
        let updated = if replace_all {
            text.replace(old, new)
        } else {
            text.replacen(old, new, 1)
        };
        tokio::fs::write(&path, updated)
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?;
        Ok(format!("edited {raw} ({count} replacement(s))"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[tokio::test]
    async fn write_file_creates_parents_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        WriteFile { spec_only: false }
            .execute(
                &json!({"path": "nested/a.txt", "content": "hello"}),
                dir.path(),
                5,
            )
            .await
            .unwrap();
        let got = fs::read_to_string(dir.path().join("nested/a.txt")).unwrap();
        assert_eq!(got, "hello");
    }

    #[tokio::test]
    async fn spec_write_allows_spec_path() {
        let dir = tempfile::tempdir().unwrap();
        WriteFile { spec_only: true }
            .execute(
                &json!({"path": "docs/edds/specification.md", "content": "spec"}),
                dir.path(),
                5,
            )
            .await
            .unwrap();
        let got = fs::read_to_string(dir.path().join("docs/edds/specification.md")).unwrap();
        assert_eq!(got, "spec");
    }

    #[tokio::test]
    async fn spec_write_allows_named_spec_file() {
        let dir = tempfile::tempdir().unwrap();
        WriteFile { spec_only: true }
            .execute(
                &json!({"path": "docs/edds/test-coverage.md", "content": "spec"}),
                dir.path(),
                5,
            )
            .await
            .unwrap();
        let got = fs::read_to_string(dir.path().join("docs/edds/test-coverage.md")).unwrap();
        assert_eq!(got, "spec");
    }

    #[tokio::test]
    async fn spec_write_rejects_non_md_under_spec_dir() {
        let dir = tempfile::tempdir().unwrap();
        let err = WriteFile { spec_only: true }
            .execute(
                &json!({"path": "docs/edds/notes.txt", "content": "nope"}),
                dir.path(),
                5,
            )
            .await;
        assert!(err.is_err());
        assert!(!dir.path().join("docs/edds/notes.txt").exists());
    }

    #[tokio::test]
    async fn spec_write_rejects_other_paths() {
        let dir = tempfile::tempdir().unwrap();
        let err = WriteFile { spec_only: true }
            .execute(
                &json!({"path": "src/main.rs", "content": "hacked"}),
                dir.path(),
                5,
            )
            .await;
        assert!(err.is_err());
        assert!(!dir.path().join("src/main.rs").exists());
    }

    #[tokio::test]
    async fn edit_file_unique_match_replaces() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "foo bar baz").unwrap();
        EditFile
            .execute(
                &json!({"path": "a.txt", "old": "bar", "new": "QUX"}),
                dir.path(),
                5,
            )
            .await
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "foo QUX baz"
        );
    }

    #[tokio::test]
    async fn edit_file_rejects_non_unique_without_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "x x x").unwrap();
        let err = EditFile
            .execute(
                &json!({"path": "a.txt", "old": "x", "new": "y"}),
                dir.path(),
                5,
            )
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn edit_file_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "x x x").unwrap();
        EditFile
            .execute(
                &json!({"path": "a.txt", "old": "x", "new": "y", "replace_all": true}),
                dir.path(),
                5,
            )
            .await
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "y y y"
        );
    }
}

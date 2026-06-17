use std::path::Path;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Safety, Tool, ToolError, resolve_in_repo};

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::BadArgs(format!("missing string '{key}'")))
}

pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &'static str {
        "write_file"
    }
    fn description(&self) -> &'static str {
        "Create or overwrite a file in the workspace with the given content."
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
        let path = resolve_in_repo(cwd, str_arg(args, "path")?)?;
        let content = str_arg(args, "content")?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::Io(e.to_string()))?;
        }
        tokio::fs::write(&path, content)
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?;
        Ok(format!(
            "wrote {} bytes to {}",
            content.len(),
            path.display()
        ))
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
        let path = resolve_in_repo(cwd, str_arg(args, "path")?)?;
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
        Ok(format!(
            "edited {} ({count} replacement(s))",
            path.display()
        ))
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
        WriteFile
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

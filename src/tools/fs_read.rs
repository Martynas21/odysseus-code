use std::path::Path;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Safety, Tool, ToolError, resolve_in_repo, str_arg, truncate};

const MAX_OUTPUT: usize = 60_000;

pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }
    fn description(&self) -> &'static str {
        "Read a UTF-8 text file from the workspace. Optional 0-based line offset and limit."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path relative to the workspace root"},
                "offset": {"type": "integer", "description": "0-based first line to include"},
                "limit": {"type": "integer", "description": "Max lines to include"}
            },
            "required": ["path"]
        })
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    async fn execute(&self, args: &Value, cwd: &Path, _t: u64) -> Result<String, ToolError> {
        let path = resolve_in_repo(cwd, str_arg(args, "path")?)?;
        let text = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?;
        let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|l| l as usize);
        let selected: String = text
            .lines()
            .skip(offset)
            .take(limit.unwrap_or(usize::MAX))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(truncate(selected, MAX_OUTPUT))
    }
}

pub struct ListDir;

#[async_trait]
impl Tool for ListDir {
    fn name(&self) -> &'static str {
        "list_dir"
    }
    fn description(&self) -> &'static str {
        "List the entries of a directory in the workspace (one level)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory relative to the workspace root"}
            },
            "required": ["path"]
        })
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    async fn execute(&self, args: &Value, cwd: &Path, _t: u64) -> Result<String, ToolError> {
        let dir = resolve_in_repo(cwd, str_arg(args, "path")?)?;
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?;
        let mut names = Vec::new();
        while let Some(e) = entries
            .next_entry()
            .await
            .map_err(|e| ToolError::Io(e.to_string()))?
        {
            let suffix = if e.path().is_dir() { "/" } else { "" };
            names.push(format!("{}{suffix}", e.file_name().to_string_lossy()));
        }
        names.sort();
        Ok(truncate(names.join("\n"), MAX_OUTPUT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[tokio::test]
    async fn read_file_returns_contents() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "line1\nline2\n").unwrap();
        let out = ReadFile
            .execute(&json!({"path": "a.txt"}), dir.path(), 5)
            .await
            .unwrap();
        assert!(out.contains("line1"));
        assert!(out.contains("line2"));
    }

    #[tokio::test]
    async fn read_file_offset_limit() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "1\n2\n3\n4\n").unwrap();
        let out = ReadFile
            .execute(
                &json!({"path": "a.txt", "offset": 1, "limit": 2}),
                dir.path(),
                5,
            )
            .await
            .unwrap();
        assert!(out.contains("2") && out.contains("3"));
        assert!(!out.contains("4"));
    }

    #[tokio::test]
    async fn read_file_is_consistent_across_offset_args() {
        // A full read and an explicit offset:0 read must return identical
        // content (same normalization), so a later edit_file `old` built from
        // either can't fail to match the other.
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "alpha\r\nbeta\n").unwrap();
        let full = ReadFile
            .execute(&json!({"path": "a.txt"}), dir.path(), 5)
            .await
            .unwrap();
        let sliced = ReadFile
            .execute(&json!({"path": "a.txt", "offset": 0}), dir.path(), 5)
            .await
            .unwrap();
        assert_eq!(full, sliced);
    }

    #[tokio::test]
    async fn list_dir_lists_entries() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        let out = ListDir
            .execute(&json!({"path": "."}), dir.path(), 5)
            .await
            .unwrap();
        assert!(out.contains("a.rs"));
        assert!(out.contains("sub"));
    }
}

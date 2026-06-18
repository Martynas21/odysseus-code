use std::path::Path;

use async_trait::async_trait;
use regex::Regex;
use serde_json::{Value, json};
use walkdir::WalkDir;

use super::{Safety, Tool, ToolError, str_arg, truncate};

const MAX_OUTPUT: usize = 40_000;
const MAX_MATCHES: usize = 500;

pub struct Grep;

#[async_trait]
impl Tool for Grep {
    fn name(&self) -> &'static str {
        "grep"
    }
    fn description(&self) -> &'static str {
        "Search the workspace for lines matching a regular expression. Returns path:line: text."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Rust regex syntax"}
            },
            "required": ["pattern"]
        })
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    async fn execute(&self, args: &Value, cwd: &Path, _t: u64) -> Result<String, ToolError> {
        let pattern = str_arg(args, "pattern")?;
        let re = Regex::new(pattern).map_err(|e| ToolError::BadArgs(e.to_string()))?;
        let root = cwd.to_path_buf();
        let out = tokio::task::spawn_blocking(move || {
            let mut hits = Vec::new();
            for entry in WalkDir::new(&root).into_iter().filter_map(Result::ok) {
                if hits.len() >= MAX_MATCHES {
                    break;
                }
                if !entry.file_type().is_file() {
                    continue;
                }
                if entry.path().components().any(|c| c.as_os_str() == ".git") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(entry.path()) else {
                    continue;
                };
                let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
                for (n, line) in text.lines().enumerate() {
                    if re.is_match(line) {
                        hits.push(format!("{}:{}: {}", rel.display(), n + 1, line.trim_end()));
                        if hits.len() >= MAX_MATCHES {
                            break;
                        }
                    }
                }
            }
            hits.join("\n")
        })
        .await
        .map_err(|e| ToolError::Failed(e.to_string()))?;
        if out.is_empty() {
            Ok("no matches".into())
        } else {
            Ok(truncate(out, MAX_OUTPUT))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[tokio::test]
    async fn grep_finds_matches_with_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "fn main() {}\nlet x = 1;\n").unwrap();
        fs::write(dir.path().join("b.rs"), "no match here\n").unwrap();
        let out = Grep
            .execute(&json!({"pattern": "fn \\w+"}), dir.path(), 5)
            .await
            .unwrap();
        assert!(out.contains("a.rs"));
        assert!(out.contains("fn main"));
        assert!(!out.contains("b.rs"));
    }

    #[tokio::test]
    async fn grep_bad_regex_errors() {
        let dir = tempfile::tempdir().unwrap();
        let err = Grep.execute(&json!({"pattern": "("}), dir.path(), 5).await;
        assert!(err.is_err());
    }
}

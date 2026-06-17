use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Safety, Tool, ToolError, truncate};

const MAX_OUTPUT: usize = 40_000;
const MAX_TIMEOUT_SECS: u64 = 600;

pub struct Shell;

#[async_trait]
impl Tool for Shell {
    fn name(&self) -> &'static str {
        "shell"
    }
    fn description(&self) -> &'static str {
        "Run a shell command (sh -c) in the workspace. Returns combined stdout/stderr and exit code."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "cmd": {"type": "string", "description": "The command line to run"}
            },
            "required": ["cmd"]
        })
    }
    fn safety(&self) -> Safety {
        Safety::Mutating
    }
    async fn execute(
        &self,
        args: &Value,
        cwd: &Path,
        timeout_secs: u64,
    ) -> Result<String, ToolError> {
        let cmd = args
            .get("cmd")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::BadArgs("missing string 'cmd'".into()))?;
        let secs = timeout_secs.clamp(1, MAX_TIMEOUT_SECS);
        let child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();
        let output = tokio::time::timeout(Duration::from_secs(secs), child)
            .await
            .map_err(|_| ToolError::Failed(format!("command timed out after {secs}s")))?
            .map_err(|e| ToolError::Io(e.to_string()))?;
        let code = output.status.code().unwrap_or(-1);
        let mut combined = String::new();
        combined.push_str(&String::from_utf8_lossy(&output.stdout));
        if !output.stderr.is_empty() {
            combined.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        Ok(truncate(format!("{combined}\nexit: {code}"), MAX_OUTPUT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn shell_runs_and_captures_stdout() {
        let dir = tempfile::tempdir().unwrap();
        let out = Shell
            .execute(&json!({"cmd": "echo hello"}), dir.path(), 5)
            .await
            .unwrap();
        assert!(out.contains("hello"));
        assert!(out.contains("exit: 0"));
    }

    #[tokio::test]
    async fn shell_reports_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let out = Shell
            .execute(&json!({"cmd": "exit 3"}), dir.path(), 5)
            .await
            .unwrap();
        assert!(out.contains("exit: 3"));
    }

    #[tokio::test]
    async fn shell_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let err = Shell
            .execute(&json!({"cmd": "sleep 5"}), dir.path(), 1)
            .await;
        assert!(err.is_err());
    }
}

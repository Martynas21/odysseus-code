//! Shell tool. Task 4.1 stub: real metadata, `execute` is a placeholder
//! until Task 4.5 implements `shell`.

use std::path::Path;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Safety, Tool, ToolError};

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
        json!({ "type": "object" })
    }
    fn safety(&self) -> Safety {
        Safety::Mutating
    }
    async fn execute(&self, _args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        Err(ToolError::Failed("unimplemented".into()))
    }
}

//! Search tool. Task 4.1 stub: real metadata, `execute` is a placeholder
//! until Task 4.4 implements `grep`.

use std::path::Path;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Safety, Tool, ToolError};

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
        json!({ "type": "object" })
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    async fn execute(&self, _args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        Err(ToolError::Failed("unimplemented".into()))
    }
}

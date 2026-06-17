//! Read-only filesystem tools. Task 4.1 stub: real metadata, `execute` is a
//! placeholder until Task 4.2 implements `read_file` and `list_dir`.

use std::path::Path;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Safety, Tool, ToolError};

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
        json!({ "type": "object" })
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    async fn execute(&self, _args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        Err(ToolError::Failed("unimplemented".into()))
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
        json!({ "type": "object" })
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    async fn execute(&self, _args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        Err(ToolError::Failed("unimplemented".into()))
    }
}

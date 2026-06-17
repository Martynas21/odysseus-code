//! File-mutating tools. Task 4.1 stub: real metadata, `execute` is a
//! placeholder until Task 4.3 implements `write_file` and `edit_file`.

use std::path::Path;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Safety, Tool, ToolError};

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
        json!({ "type": "object" })
    }
    fn safety(&self) -> Safety {
        Safety::Mutating
    }
    async fn execute(&self, _args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        Err(ToolError::Failed("unimplemented".into()))
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
        json!({ "type": "object" })
    }
    fn safety(&self) -> Safety {
        Safety::Mutating
    }
    async fn execute(&self, _args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        Err(ToolError::Failed("unimplemented".into()))
    }
}

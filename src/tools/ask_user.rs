use std::path::Path;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{Safety, Tool, ToolError};

pub struct AskUser;

#[async_trait]
impl Tool for AskUser {
    fn name(&self) -> &'static str {
        "ask_user"
    }
    fn description(&self) -> &'static str {
        "Ask the user ONE clarifying question with 2-3 short selectable options. The user may \
         pick an option, answer freeform, or annotate an option. Use it to gather context one \
         step at a time."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {"type": "string", "description": "The single question to ask the user"},
                "options": {
                    "type": "array",
                    "description": "2-3 short selectable options",
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": {"type": "string", "description": "Short option label"},
                            "description": {"type": "string", "description": "Optional clarification of the option"}
                        },
                        "required": ["label"]
                    }
                }
            },
            "required": ["question", "options"]
        })
    }
    fn safety(&self) -> Safety {
        Safety::ReadOnly
    }
    fn interactive(&self) -> bool {
        true
    }
    /// Unreachable: the agent loop dispatches interactive tools (see
    /// `Tool::interactive`) by blocking for user input before it would ever call
    /// `execute`. Kept only to satisfy the trait, with a guard error in case the
    /// dispatch is ever bypassed.
    async fn execute(&self, _args: &Value, _cwd: &Path, _t: u64) -> Result<String, ToolError> {
        Err(ToolError::Failed("ask_user is handled interactively".into()))
    }
}

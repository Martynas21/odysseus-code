use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    fn wire(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// One reassembled tool call. `arguments` is the raw JSON string the model
/// emitted (parsed only at execution time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// A single conversation turn. Mirrors the OpenAI chat message shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::plain(Role::System, content)
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self::plain(Role::User, content)
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::plain(Role::Assistant, content)
    }
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }
    fn plain(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// Serialize to the OpenAI `/v1/chat/completions` message wire shape.
    pub fn to_wire(&self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("role".into(), self.role.wire().into());
        obj.insert("content".into(), self.content.clone().into());
        if !self.tool_calls.is_empty() {
            let calls: Vec<Value> = self
                .tool_calls
                .iter()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "type": "function",
                        "function": {"name": c.name, "arguments": c.arguments}
                    })
                })
                .collect();
            obj.insert("tool_calls".into(), Value::Array(calls));
        }
        if let Some(id) = &self.tool_call_id {
            obj.insert("tool_call_id".into(), id.clone().into());
        }
        Value::Object(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn user_message_to_wire_is_role_and_content() {
        let m = ChatMessage::user("hello");
        assert_eq!(m.to_wire(), json!({"role": "user", "content": "hello"}));
    }

    #[test]
    fn assistant_with_tool_calls_serializes_function_shape() {
        let m = ChatMessage {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "shell".into(),
                arguments: r#"{"cmd":"ls"}"#.into(),
            }],
            tool_call_id: None,
        };
        assert_eq!(
            m.to_wire(),
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "shell", "arguments": r#"{"cmd":"ls"}"#}
                }]
            })
        );
    }

    #[test]
    fn tool_result_to_wire_carries_tool_call_id() {
        let m = ChatMessage::tool_result("call_1", "ok");
        assert_eq!(
            m.to_wire(),
            json!({"role": "tool", "content": "ok", "tool_call_id": "call_1"})
        );
    }
}

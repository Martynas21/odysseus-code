pub mod message;
pub mod sse;

pub use message::{ChatMessage, Role, ToolCall};

pub mod openai;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use serde_json::{Value, json};
use thiserror::Error;

/// A tool definition advertised to the model.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value, // JSON schema
}

impl ToolDef {
    fn to_wire(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }
}

/// One streamed completion request.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    pub temperature: f32,
    pub max_tokens: u32,
}

impl ChatRequest {
    pub fn to_body(&self) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": self.messages.iter().map(ChatMessage::to_wire).collect::<Vec<_>>(),
            "temperature": self.temperature,
            "max_tokens": self.max_tokens,
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        if !self.tools.is_empty() {
            body["tools"] = Value::Array(self.tools.iter().map(ToolDef::to_wire).collect());
            body["tool_choice"] = "auto".into();
        }
        body
    }
}

/// One streamed event from a provider.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    TextDelta(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    Done,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("not authorized (HTTP 401) — check `config set api_key`")]
    Unauthorized,
    #[error("rate limited (HTTP 429); gave up after one retry")]
    RateLimited,
    #[error("server returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("could not reach {url}: {source}")]
    Network { url: String, source: reqwest::Error },
    #[error("bad stream: {0}")]
    BadStream(String),
}

#[async_trait]
pub trait Provider: Send + Sync {
    /// Open a streamed completion. Errors raised here are connection/open-time
    /// failures; mid-stream failures arrive as `Err` items in the stream.
    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamEvent, ProviderError>>, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_request_body_has_stream_and_tools() {
        let req = ChatRequest {
            model: "qwen3".into(),
            messages: vec![ChatMessage::user("hi")],
            tools: vec![ToolDef {
                name: "shell".into(),
                description: "run a shell command".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
            temperature: 0.2,
            max_tokens: 4096,
        };
        let body = req.to_body();
        assert_eq!(body["model"], "qwen3");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["messages"][0]["content"], "hi");
        assert_eq!(body["tools"][0]["function"]["name"], "shell");
    }

    #[test]
    fn empty_tools_omits_tools_field() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![],
            tools: vec![],
            temperature: 0.0,
            max_tokens: 1,
        };
        let body = req.to_body();
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
    }
}

pub mod message;
pub mod sse;

// `Role`/`ToolCall` are part of the crate's llm surface and are consumed by the
// agent loop in later phases; re-exported here for that use.
#[allow(unused_imports)]
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
    /// Whether to let a reasoning model think. When false, ask the server to
    /// skip the chain-of-thought (qwen3's `enable_thinking` soft-switch).
    pub think: bool,
}

impl ChatRequest {
    pub fn to_body(&self) -> Value {
        let mut messages: Vec<Value> = self.messages.iter().map(ChatMessage::to_wire).collect();
        if !self.think {
            // Inject qwen3's `/no_think` soft-switch into the prompt. This is the
            // robust path for servers (e.g. LM Studio) that ignore the
            // `chat_template_kwargs` param set below. Apply to the latest user
            // message, falling back to the system message.
            let idx = messages
                .iter()
                .rposition(|m| m["role"] == "user")
                .or_else(|| messages.iter().position(|m| m["role"] == "system"));
            if let Some(i) = idx
                && let Some(content) = messages[i]["content"].as_str()
            {
                messages[i]["content"] = format!("{content} /no_think").into();
            }
        }
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "temperature": self.temperature,
            "max_tokens": self.max_tokens,
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        if !self.tools.is_empty() {
            body["tools"] = Value::Array(self.tools.iter().map(ToolDef::to_wire).collect());
            body["tool_choice"] = "auto".into();
        }
        if !self.think {
            // Belt-and-suspenders: also send the documented param for servers
            // that honor it (vLLM, etc.).
            body["chat_template_kwargs"] = json!({"enable_thinking": false});
        }
        body
    }
}

/// One streamed event from a provider.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    TextDelta(String),
    /// A chunk of the model's chain-of-thought (reasoning models only).
    ReasoningDelta(String),
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
            think: true,
        };
        let body = req.to_body();
        assert_eq!(body["model"], "qwen3");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["messages"][0]["content"], "hi");
        assert_eq!(body["tools"][0]["function"]["name"], "shell");
        // Thinking on → no enable_thinking override sent.
        assert!(body.get("chat_template_kwargs").is_none());
    }

    #[test]
    fn empty_tools_omits_tools_field() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![],
            tools: vec![],
            temperature: 0.0,
            max_tokens: 1,
            think: true,
        };
        let body = req.to_body();
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn no_think_disables_thinking_via_param_and_prompt_token() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![ChatMessage::system("sys"), ChatMessage::user("hi")],
            tools: vec![],
            temperature: 0.0,
            max_tokens: 1,
            think: false,
        };
        let body = req.to_body();
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        // The `/no_think` soft-switch is appended to the latest user message.
        assert_eq!(body["messages"][1]["content"], "hi /no_think");
        // The system message is untouched.
        assert_eq!(body["messages"][0]["content"], "sys");
    }
}

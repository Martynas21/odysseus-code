use std::path::Path;
use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::llm::message::{ChatMessage, Role};
use crate::llm::{ChatRequest, Provider, StreamEvent};
use crate::tools::{Safety, ToolRegistry};

mod assembler;
use assembler::ToolCallAssembler;

const MAX_ITERATIONS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    Prompt,
    Auto,
    ReadOnly,
}

impl ApprovalPolicy {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "auto" => ApprovalPolicy::Auto,
            "readonly" => ApprovalPolicy::ReadOnly,
            _ => ApprovalPolicy::Prompt,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approve,
    ApproveAlways,
    Deny,
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    AssistantTextDelta(String),
    ReasoningDelta(String),
    AssistantTextDone,
    ToolCallRequested {
        name: String,
        args: String,
    },
    ApprovalRequired {
        name: String,
        args: String,
    },
    ToolStarted {
        name: String,
    },
    ToolFinished {
        name: String,
        output: String,
        ok: bool,
    },
    Error(String),
    Done,
    TurnComplete(Vec<ChatMessage>),
}

#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    provider: Arc<dyn Provider>,
    registry: Arc<ToolRegistry>,
    mut history: Vec<ChatMessage>,
    ev_tx: mpsc::UnboundedSender<AgentEvent>,
    mut approvals: mpsc::UnboundedReceiver<ApprovalDecision>,
    cfg: &Config,
    cwd: &Path,
    mut policy: ApprovalPolicy,
    think: bool,
) -> Vec<ChatMessage> {
    let base_len = history.len();
    let tools = registry.defs();

    for _ in 0..MAX_ITERATIONS {
        let mut messages = history.clone();
        if let Some(status) = registry.tracker().status_text() {
            messages.push(ChatMessage::system(status));
        }
        let req = ChatRequest {
            model: cfg.model.clone(),
            messages,
            tools: tools.clone(),
            temperature: cfg.temperature,
            max_tokens: cfg.max_tokens,
            think,
        };

        let mut stream = match provider.chat_stream(req).await {
            Ok(s) => s,
            Err(e) => {
                let _ = ev_tx.send(AgentEvent::Error(e.to_string()));
                return history.split_off(base_len);
            }
        };

        let mut text = String::new();
        let mut assembler = ToolCallAssembler::default();
        while let Some(item) = stream.next().await {
            match item {
                Ok(StreamEvent::TextDelta(d)) => {
                    text.push_str(&d);
                    let _ = ev_tx.send(AgentEvent::AssistantTextDelta(d));
                }
                Ok(StreamEvent::ReasoningDelta(d)) => {
                    let _ = ev_tx.send(AgentEvent::ReasoningDelta(d));
                }
                Ok(StreamEvent::ToolCallDelta {
                    index,
                    id,
                    name,
                    arguments,
                }) => {
                    assembler.push(index, id, name, &arguments);
                }
                Ok(StreamEvent::Usage { .. }) => {}
                Ok(StreamEvent::Done) => break,
                Err(e) => {
                    let _ = ev_tx.send(AgentEvent::Error(e.to_string()));
                    history.push(ChatMessage {
                        role: Role::Assistant,
                        content: text,
                        tool_calls: Vec::new(),
                        tool_call_id: None,
                    });
                    return history.split_off(base_len);
                }
            }
        }

        if !text.is_empty() {
            let _ = ev_tx.send(AgentEvent::AssistantTextDone);
        }

        let calls = assembler.finish();
        history.push(ChatMessage {
            role: Role::Assistant,
            content: text,
            tool_calls: calls.clone(),
            tool_call_id: None,
        });

        if calls.is_empty() {
            let _ = ev_tx.send(AgentEvent::Done);
            return history.split_off(base_len);
        }

        for call in calls {
            let _ = ev_tx.send(AgentEvent::ToolCallRequested {
                name: call.name.clone(),
                args: call.arguments.clone(),
            });

            let Some(tool) = registry.get(&call.name) else {
                let msg = format!("unknown tool '{}'", call.name);
                history.push(ChatMessage::tool_result(&call.id, &msg));
                let _ = ev_tx.send(AgentEvent::ToolFinished {
                    name: call.name.clone(),
                    output: msg,
                    ok: false,
                });
                continue;
            };
            let safety = tool.safety();
            let allowed = match (safety, policy) {
                (Safety::ReadOnly, _) | (_, ApprovalPolicy::Auto) => true,
                (Safety::Mutating, ApprovalPolicy::ReadOnly) => false,
                (Safety::Mutating, ApprovalPolicy::Prompt) => {
                    let _ = ev_tx.send(AgentEvent::ApprovalRequired {
                        name: call.name.clone(),
                        args: call.arguments.clone(),
                    });
                    match approvals.recv().await {
                        Some(ApprovalDecision::Approve) => true,
                        Some(ApprovalDecision::ApproveAlways) => {
                            policy = ApprovalPolicy::Auto;
                            true
                        }
                        Some(ApprovalDecision::Deny) | None => false,
                    }
                }
            };

            if !allowed {
                history.push(ChatMessage::tool_result(
                    &call.id,
                    "The user denied this tool call.",
                ));
                let _ = ev_tx.send(AgentEvent::ToolFinished {
                    name: call.name.clone(),
                    output: "denied".into(),
                    ok: false,
                });
                continue;
            }

            let _ = ev_tx.send(AgentEvent::ToolStarted {
                name: call.name.clone(),
            });

            let args: serde_json::Value =
                serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
            let result = tool.execute(&args, cwd, cfg.tool_timeout_secs).await;
            let (output, ok) = match result {
                Ok(out) => (out, true),
                Err(e) => (e.to_string(), false),
            };
            history.push(ChatMessage::tool_result(&call.id, &output));
            let _ = ev_tx.send(AgentEvent::ToolFinished {
                name: call.name.clone(),
                output,
                ok,
            });
        }
    }

    let _ = ev_tx.send(AgentEvent::Error(format!(
        "agent exceeded {MAX_ITERATIONS} iterations without finishing"
    )));
    history.split_off(base_len)
}

#[cfg(test)]
mod tests;

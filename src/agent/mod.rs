//! The agent loop. Phase-3 stub: a single streaming turn with no tools.
//! Task 5.2 replaces `run_agent` with the full tool-calling loop; the signature
//! here is final so the swap-in needs no caller changes.

use std::path::Path;
use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::config::Config;
use crate::llm::message::ChatMessage;
use crate::llm::{ChatRequest, Provider, StreamEvent};
use crate::tools::ToolRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    /// Prompt before mutating tools; auto-run read-only.
    Prompt,
    /// Run everything without prompting.
    Auto,
    /// Auto-run read-only; auto-deny mutating.
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
    AssistantTextDone,
    ToolCallRequested {
        id: String,
        name: String,
        args: String,
    },
    ApprovalRequired {
        id: String,
        name: String,
        args: String,
    },
    ToolStarted {
        id: String,
        name: String,
    },
    ToolFinished {
        id: String,
        name: String,
        output: String,
        ok: bool,
    },
    Error(String),
    Done,
    /// Terminal message carrying the new turns to splice into UI history.
    TurnComplete(Vec<ChatMessage>),
}

/// Phase-3 stub: open one completion, stream assistant text, and return the
/// assistant turn. Tool-call deltas are ignored; the full loop arrives in
/// Task 5.2 (which replaces this function wholesale).
#[allow(unused_variables)]
#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    provider: Arc<dyn Provider>,
    registry: Arc<ToolRegistry>,
    history: Vec<ChatMessage>,
    ev_tx: UnboundedSender<AgentEvent>,
    approvals: UnboundedReceiver<ApprovalDecision>,
    cfg: &Config,
    cwd: &Path,
    policy: ApprovalPolicy,
) -> Vec<ChatMessage> {
    // These become load-bearing once tools land (Task 5.2); silence dead-param
    // lints without changing the final signature.
    let _ = (&registry, &approvals, cwd, policy);

    let req = ChatRequest {
        model: cfg.model.clone(),
        messages: history,
        tools: Vec::new(),
        temperature: cfg.temperature,
        max_tokens: cfg.max_tokens,
    };

    let mut stream = match provider.chat_stream(req).await {
        Ok(stream) => stream,
        Err(err) => {
            let _ = ev_tx.send(AgentEvent::Error(err.to_string()));
            return Vec::new();
        }
    };

    let mut text = String::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(StreamEvent::TextDelta(delta)) => {
                text.push_str(&delta);
                let _ = ev_tx.send(AgentEvent::AssistantTextDelta(delta));
            }
            Ok(StreamEvent::Done) => break,
            // Tool calls and usage are ignored until Phase 5.
            Ok(_) => {}
            Err(err) => {
                let _ = ev_tx.send(AgentEvent::Error(err.to_string()));
                return Vec::new();
            }
        }
    }

    let _ = ev_tx.send(AgentEvent::AssistantTextDone);
    let _ = ev_tx.send(AgentEvent::Done);

    vec![ChatMessage::assistant(text)]
}

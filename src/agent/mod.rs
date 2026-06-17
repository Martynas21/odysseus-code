//! The agent loop: stream assistant text, reassemble tool calls, execute them
//! (gating mutating tools per the approval policy), and loop until the model
//! answers with no tool calls.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::llm::message::{ChatMessage, Role, ToolCall};
use crate::llm::{ChatRequest, Provider, StreamEvent};
use crate::tools::{Safety, ToolRegistry};

/// Give up after this many model turns in a single user request.
const MAX_ITERATIONS: usize = 16;

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

// Several event `id`/`name` fields are read by the Phase 6 approval UI and the
// Phase 7 tool-display polish (Tasks 6.1/7.1); remove this allow once consumed.
#[allow(dead_code)]
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

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

/// Reassembles streamed tool-call fragments keyed by their `index`.
#[derive(Default)]
pub struct ToolCallAssembler {
    calls: BTreeMap<usize, PartialCall>,
}

impl ToolCallAssembler {
    pub fn push(&mut self, index: usize, id: Option<String>, name: Option<String>, args: &str) {
        let call = self.calls.entry(index).or_default();
        if let Some(id) = id
            && !id.is_empty()
        {
            call.id = id;
        }
        if let Some(name) = name
            && !name.is_empty()
        {
            call.name = name;
        }
        call.arguments.push_str(args);
    }

    pub fn finish(self) -> Vec<ToolCall> {
        self.calls
            .into_values()
            .map(|c| ToolCall {
                id: c.id,
                name: c.name,
                arguments: c.arguments,
            })
            .collect()
    }
}

/// Run one user turn to completion: stream assistant text, reassemble tool
/// calls, execute them (gating mutating tools per `policy`), and loop until the
/// model answers with no tool calls. Returns the new turns to splice into the
/// caller's history. Errors are reported via `AgentEvent::Error`.
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
) -> Vec<ChatMessage> {
    let base_len = history.len();

    for _ in 0..MAX_ITERATIONS {
        let req = ChatRequest {
            model: cfg.model.clone(),
            messages: history.clone(),
            tools: registry.defs(),
            temperature: cfg.temperature,
            max_tokens: cfg.max_tokens,
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
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.arguments.clone(),
            });

            let safety = registry.safety(&call.name).unwrap_or(Safety::Mutating);
            let allowed = match (safety, policy) {
                (Safety::ReadOnly, _) | (_, ApprovalPolicy::Auto) => true,
                (Safety::Mutating, ApprovalPolicy::ReadOnly) => false,
                (Safety::Mutating, ApprovalPolicy::Prompt) => {
                    let _ = ev_tx.send(AgentEvent::ApprovalRequired {
                        id: call.id.clone(),
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
                    id: call.id.clone(),
                    name: call.name.clone(),
                    output: "denied".into(),
                    ok: false,
                });
                continue;
            }

            let _ = ev_tx.send(AgentEvent::ToolStarted {
                id: call.id.clone(),
                name: call.name.clone(),
            });
            // Broken JSON args fall through to `Null`; the tool's own arg
            // validation then rejects it and the error becomes the tool result,
            // so the model still gets actionable feedback (no silent failure).
            let args: serde_json::Value =
                serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
            let result = match registry.get(&call.name) {
                Some(tool) => tool.execute(&args, cwd, cfg.tool_timeout_secs).await,
                None => Err(crate::tools::ToolError::Failed(format!(
                    "unknown tool '{}'",
                    call.name
                ))),
            };
            let (output, ok) = match result {
                Ok(out) => (out, true),
                Err(e) => (e.to_string(), false),
            };
            history.push(ChatMessage::tool_result(&call.id, &output));
            let _ = ev_tx.send(AgentEvent::ToolFinished {
                id: call.id.clone(),
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
mod tests {
    use super::*;
    use crate::llm::message::Role;
    use crate::llm::{ProviderError, ToolCall};
    use async_trait::async_trait;
    use futures_util::stream::{self, BoxStream};
    use std::sync::Mutex;

    #[test]
    fn assembler_reassembles_fragmented_call() {
        let mut a = ToolCallAssembler::default();
        a.push(0, Some("call_1".into()), Some("shell".into()), "{\"cmd\":");
        a.push(0, None, None, "\"ls\"}");
        let calls = a.finish();
        assert_eq!(
            calls,
            vec![ToolCall {
                id: "call_1".into(),
                name: "shell".into(),
                arguments: r#"{"cmd":"ls"}"#.into()
            }]
        );
    }

    #[test]
    fn assembler_keeps_parallel_calls_by_index() {
        let mut a = ToolCallAssembler::default();
        a.push(0, Some("a".into()), Some("read_file".into()), "{}");
        a.push(1, Some("b".into()), Some("grep".into()), "{}");
        let calls = a.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "a");
        assert_eq!(calls[1].id, "b");
    }

    #[test]
    fn policy_from_str() {
        assert_eq!(ApprovalPolicy::from_str("auto"), ApprovalPolicy::Auto);
        assert_eq!(
            ApprovalPolicy::from_str("readonly"),
            ApprovalPolicy::ReadOnly
        );
        assert_eq!(
            ApprovalPolicy::from_str("anything-else"),
            ApprovalPolicy::Prompt
        );
    }

    /// A provider that replays canned event scripts, one per turn.
    struct ScriptedProvider {
        turns: Mutex<std::collections::VecDeque<Vec<StreamEvent>>>,
    }
    impl ScriptedProvider {
        fn new(turns: Vec<Vec<StreamEvent>>) -> Self {
            Self {
                turns: Mutex::new(turns.into()),
            }
        }
    }
    #[async_trait]
    impl Provider for ScriptedProvider {
        async fn chat_stream(
            &self,
            _req: ChatRequest,
        ) -> Result<BoxStream<'static, Result<StreamEvent, ProviderError>>, ProviderError> {
            let events = self
                .turns
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| vec![StreamEvent::Done]);
            Ok(stream::iter(events.into_iter().map(Ok)).boxed())
        }
    }

    fn cfg() -> Config {
        Config::default()
    }

    #[tokio::test]
    async fn plain_answer_emits_text_and_done() {
        let provider = Arc::new(ScriptedProvider::new(vec![vec![
            StreamEvent::TextDelta("Hi".into()),
            StreamEvent::TextDelta(" there".into()),
            StreamEvent::Done,
        ]]));
        let registry = Arc::new(ToolRegistry::default_set());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_atx, arx) = mpsc::unbounded_channel();
        let history = vec![ChatMessage::system("sys"), ChatMessage::user("hi")];

        let new = run_agent(
            provider,
            registry,
            history,
            tx,
            arx,
            &cfg(),
            Path::new("."),
            ApprovalPolicy::Auto,
        )
        .await;

        let mut texts = String::new();
        let mut saw_done = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                AgentEvent::AssistantTextDelta(d) => texts.push_str(&d),
                AgentEvent::Done => saw_done = true,
                _ => {}
            }
        }
        assert_eq!(texts, "Hi there");
        assert!(saw_done);
        assert_eq!(new.last().unwrap().role, Role::Assistant);
        assert_eq!(new.last().unwrap().content, "Hi there");
    }

    #[tokio::test]
    async fn read_only_tool_round_trip_auto_runs() {
        // Turn 1: model asks to list_dir. Turn 2: model answers.
        let provider = Arc::new(ScriptedProvider::new(vec![
            vec![
                StreamEvent::ToolCallDelta {
                    index: 0,
                    id: Some("c1".into()),
                    name: Some("list_dir".into()),
                    arguments: r#"{"path":"."}"#.into(),
                },
                StreamEvent::Done,
            ],
            vec![StreamEvent::TextDelta("done".into()), StreamEvent::Done],
        ]));
        let registry = Arc::new(ToolRegistry::default_set());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_atx, arx) = mpsc::unbounded_channel();
        let dir = tempfile::tempdir().unwrap();
        let history = vec![ChatMessage::user("look around")];

        let new = run_agent(
            provider,
            registry,
            history,
            tx,
            arx,
            &cfg(),
            dir.path(),
            ApprovalPolicy::Prompt,
        )
        .await;

        let mut started = false;
        let mut finished = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                AgentEvent::ToolStarted { name, .. } => started = name == "list_dir",
                AgentEvent::ToolFinished { ok, .. } => finished = ok,
                _ => {}
            }
        }
        assert!(started && finished);
        // history gained: assistant(tool_call) + tool(result) + assistant(done)
        assert!(new.iter().any(|m| m.role == Role::Tool));
    }

    #[tokio::test]
    async fn mutating_tool_denied_pushes_denial_message() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            vec![
                StreamEvent::ToolCallDelta {
                    index: 0,
                    id: Some("c1".into()),
                    name: Some("shell".into()),
                    arguments: r#"{"cmd":"echo hi"}"#.into(),
                },
                StreamEvent::Done,
            ],
            vec![StreamEvent::TextDelta("ok".into()), StreamEvent::Done],
        ]));
        let registry = Arc::new(ToolRegistry::default_set());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (atx, arx) = mpsc::unbounded_channel();
        atx.send(ApprovalDecision::Deny).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let history = vec![ChatMessage::user("run it")];

        let new = run_agent(
            provider,
            registry,
            history,
            tx,
            arx,
            &cfg(),
            dir.path(),
            ApprovalPolicy::Prompt,
        )
        .await;

        let mut saw_approval = false;
        while let Ok(ev) = rx.try_recv() {
            if let AgentEvent::ApprovalRequired { .. } = ev {
                saw_approval = true;
            }
        }
        assert!(saw_approval);
        let tool_msg = new.iter().find(|m| m.role == Role::Tool).unwrap();
        assert!(tool_msg.content.to_lowercase().contains("denied"));
    }

    #[tokio::test]
    async fn exceeding_max_iterations_errors() {
        // Every turn asks for a read-only tool, never answering — should bail.
        let turn = vec![
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("c".into()),
                name: Some("list_dir".into()),
                arguments: r#"{"path":"."}"#.into(),
            },
            StreamEvent::Done,
        ];
        // Always more turns than the loop will run, so it bails on the cap, not
        // on the scripted provider running dry.
        let provider = Arc::new(ScriptedProvider::new(vec![turn; MAX_ITERATIONS + 1]));
        let registry = Arc::new(ToolRegistry::default_set());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_atx, arx) = mpsc::unbounded_channel();
        let dir = tempfile::tempdir().unwrap();
        run_agent(
            provider,
            registry,
            vec![ChatMessage::user("go")],
            tx,
            arx,
            &cfg(),
            dir.path(),
            ApprovalPolicy::Auto,
        )
        .await;
        let mut saw_error = false;
        while let Ok(ev) = rx.try_recv() {
            if let AgentEvent::Error(m) = ev {
                saw_error = m.contains("exceeded");
            }
        }
        assert!(saw_error);
    }
}

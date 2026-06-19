use super::*;
use crate::llm::ProviderError;
use crate::llm::message::Role;
use crate::skills::SkillTracker;
use async_trait::async_trait;
use futures_util::stream::{self, BoxStream};
use std::sync::Mutex;

#[test]
fn parse_question_falls_back_when_label_missing() {
    let (q, opts) = parse_question(
        r#"{"question":"Pick","options":[
            {"label":"A","description":"first"},
            {"description":"only a description"},
            {}
        ]}"#,
    );
    assert_eq!(q, "Pick");
    assert_eq!(opts.len(), 3);
    assert_eq!(opts[0].label, "A");
    assert_eq!(opts[0].description.as_deref(), Some("first"));
    // missing label promotes the description to the label
    assert_eq!(opts[1].label, "only a description");
    assert_eq!(opts[1].description, None);
    // neither present → positional placeholder
    assert_eq!(opts[2].label, "Option 3");
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
    let registry = Arc::new(ToolRegistry::default_set(SkillTracker::default()));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_atx, arx) = mpsc::unbounded_channel();
    let (_qtx, qrx) = mpsc::unbounded_channel::<QuestionAnswer>();
    let history = vec![ChatMessage::system("sys"), ChatMessage::user("hi")];

    let new = run_agent(
        provider,
        registry,
        history,
        tx,
        arx,
        qrx,
        &cfg(),
        Path::new("."),
        ApprovalPolicy::Auto,
        true,
        true,
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
async fn reasoning_is_surfaced_but_excluded_from_history() {
    let provider = Arc::new(ScriptedProvider::new(vec![vec![
        StreamEvent::ReasoningDelta("thinking…".into()),
        StreamEvent::TextDelta("answer".into()),
        StreamEvent::Done,
    ]]));
    let registry = Arc::new(ToolRegistry::default_set(SkillTracker::default()));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_atx, arx) = mpsc::unbounded_channel();
    let (_qtx, qrx) = mpsc::unbounded_channel::<QuestionAnswer>();
    let history = vec![ChatMessage::user("q")];

    let new = run_agent(
        provider,
        registry,
        history,
        tx,
        arx,
        qrx,
        &cfg(),
        Path::new("."),
        ApprovalPolicy::Auto,
        true,
        true,
    )
    .await;

    let mut reasoning = String::new();
    while let Ok(ev) = rx.try_recv() {
        if let AgentEvent::ReasoningDelta(d) = ev {
            reasoning.push_str(&d);
        }
    }
    assert_eq!(reasoning, "thinking…");
    assert_eq!(new.last().unwrap().content, "answer");
}

#[tokio::test]
async fn read_only_tool_round_trip_auto_runs() {
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
    let registry = Arc::new(ToolRegistry::default_set(SkillTracker::default()));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_atx, arx) = mpsc::unbounded_channel();
    let (_qtx, qrx) = mpsc::unbounded_channel::<QuestionAnswer>();
    let dir = tempfile::tempdir().unwrap();
    let history = vec![ChatMessage::user("look around")];

    let new = run_agent(
        provider,
        registry,
        history,
        tx,
        arx,
        qrx,
        &cfg(),
        dir.path(),
        ApprovalPolicy::Prompt,
        true,
        true,
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
    let registry = Arc::new(ToolRegistry::default_set(SkillTracker::default()));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (atx, arx) = mpsc::unbounded_channel();
    let (_qtx, qrx) = mpsc::unbounded_channel::<QuestionAnswer>();
    atx.send(ApprovalDecision::Deny).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let history = vec![ChatMessage::user("run it")];

    let new = run_agent(
        provider,
        registry,
        history,
        tx,
        arx,
        qrx,
        &cfg(),
        dir.path(),
        ApprovalPolicy::Prompt,
        true,
        true,
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
    let turn = vec![
        StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("c".into()),
            name: Some("list_dir".into()),
            arguments: r#"{"path":"."}"#.into(),
        },
        StreamEvent::Done,
    ];
    let provider = Arc::new(ScriptedProvider::new(vec![turn; MAX_ITERATIONS + 1]));
    let registry = Arc::new(ToolRegistry::default_set(SkillTracker::default()));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_atx, arx) = mpsc::unbounded_channel();
    let (_qtx, qrx) = mpsc::unbounded_channel::<QuestionAnswer>();
    let dir = tempfile::tempdir().unwrap();
    run_agent(
        provider,
        registry,
        vec![ChatMessage::user("go")],
        tx,
        arx,
        qrx,
        &cfg(),
        dir.path(),
        ApprovalPolicy::Auto,
        true,
        true,
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

#[tokio::test]
async fn unknown_tool_fails_without_approval_prompt() {
    let provider = Arc::new(ScriptedProvider::new(vec![
        vec![
            StreamEvent::ToolCallDelta {
                index: 0,
                id: Some("c1".into()),
                name: Some("teleport".into()),
                arguments: "{}".into(),
            },
            StreamEvent::Done,
        ],
        vec![StreamEvent::TextDelta("ok".into()), StreamEvent::Done],
    ]));
    let registry = Arc::new(ToolRegistry::default_set(SkillTracker::default()));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_atx, arx) = mpsc::unbounded_channel();
    let (_qtx, qrx) = mpsc::unbounded_channel::<QuestionAnswer>();
    let dir = tempfile::tempdir().unwrap();

    let new = run_agent(
        provider,
        registry,
        vec![ChatMessage::user("do it")],
        tx,
        arx,
        qrx,
        &cfg(),
        dir.path(),
        ApprovalPolicy::Prompt,
        true,
        true,
    )
    .await;

    let mut saw_approval = false;
    let mut failed_unknown = false;
    while let Ok(ev) = rx.try_recv() {
        match ev {
            AgentEvent::ApprovalRequired { .. } => saw_approval = true,
            AgentEvent::ToolFinished { ok, output, .. } => {
                failed_unknown = !ok && output.contains("unknown tool");
            }
            _ => {}
        }
    }
    assert!(!saw_approval, "must not prompt approval for unknown tool");
    assert!(failed_unknown);
    let tool_msg = new.iter().find(|m| m.role == Role::Tool).unwrap();
    assert!(tool_msg.content.contains("unknown tool"));
}

struct CapturingProvider {
    captured: Arc<Mutex<Vec<ChatMessage>>>,
}
#[async_trait]
impl Provider for CapturingProvider {
    async fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamEvent, ProviderError>>, ProviderError> {
        *self.captured.lock().unwrap() = req.messages.clone();
        Ok(stream::iter(vec![Ok(StreamEvent::Done)]).boxed())
    }
}

fn captured_text(captured: &Arc<Mutex<Vec<ChatMessage>>>) -> String {
    captured
        .lock()
        .unwrap()
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn active_skill_status_is_injected_into_request() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(CapturingProvider {
        captured: captured.clone(),
    });
    let tracker = SkillTracker::default();
    tracker.start(
        "summarize-changes",
        &["Inspect changes".to_string(), "Write summary".to_string()],
    );
    let registry = Arc::new(ToolRegistry::default_set(tracker));
    let (tx, _rx) = mpsc::unbounded_channel();
    let (_atx, arx) = mpsc::unbounded_channel();
    let (_qtx, qrx) = mpsc::unbounded_channel::<QuestionAnswer>();

    run_agent(
        provider,
        registry,
        vec![ChatMessage::user("hi")],
        tx,
        arx,
        qrx,
        &cfg(),
        Path::new("."),
        ApprovalPolicy::Auto,
        true,
        false,
    )
    .await;

    let text = captured_text(&captured);
    assert!(
        text.contains("Skill in progress: summarize-changes"),
        "pinned status missing from request: {text}"
    );
    assert!(text.contains("Inspect changes"));
}

#[tokio::test]
async fn no_status_injected_without_active_skill() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let provider = Arc::new(CapturingProvider {
        captured: captured.clone(),
    });
    let registry = Arc::new(ToolRegistry::default_set(SkillTracker::default()));
    let (tx, _rx) = mpsc::unbounded_channel();
    let (_atx, arx) = mpsc::unbounded_channel();
    let (_qtx, qrx) = mpsc::unbounded_channel::<QuestionAnswer>();

    run_agent(
        provider,
        registry,
        vec![ChatMessage::user("hi")],
        tx,
        arx,
        qrx,
        &cfg(),
        Path::new("."),
        ApprovalPolicy::Auto,
        true,
        false,
    )
    .await;

    assert!(!captured_text(&captured).contains("Skill in progress"));
}

struct MidStreamErrorProvider;
#[async_trait]
impl Provider for MidStreamErrorProvider {
    async fn chat_stream(
        &self,
        _req: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamEvent, ProviderError>>, ProviderError> {
        let items: Vec<Result<StreamEvent, ProviderError>> = vec![
            Ok(StreamEvent::TextDelta("partial".into())),
            Err(ProviderError::BadStream("reset".into())),
        ];
        Ok(stream::iter(items).boxed())
    }
}

#[tokio::test]
async fn mid_stream_error_keeps_role_alternation() {
    let provider = Arc::new(MidStreamErrorProvider);
    let registry = Arc::new(ToolRegistry::default_set(SkillTracker::default()));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_atx, arx) = mpsc::unbounded_channel();
    let (_qtx, qrx) = mpsc::unbounded_channel::<QuestionAnswer>();

    let new = run_agent(
        provider,
        registry,
        vec![ChatMessage::system("sys"), ChatMessage::user("hi")],
        tx,
        arx,
        qrx,
        &cfg(),
        Path::new("."),
        ApprovalPolicy::Auto,
        true,
        true,
    )
    .await;

    let mut saw_error = false;
    while let Ok(ev) = rx.try_recv() {
        if matches!(ev, AgentEvent::Error(_)) {
            saw_error = true;
        }
    }
    assert!(saw_error);
    let last = new.last().unwrap();
    assert_eq!(last.role, Role::Assistant);
    assert_eq!(last.content, "partial");
}

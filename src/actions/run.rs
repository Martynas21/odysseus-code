//! Headless `run` subcommand: execute one agent turn non-interactively,
//! streaming assistant text to stdout and tool activity to stderr.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, bail};
use tokio::sync::mpsc;

use crate::agent::{self, AgentEvent, ApprovalPolicy};
use crate::config::Config;
use crate::context::PromptContext;
use crate::llm::Provider;
use crate::llm::message::ChatMessage;
use crate::llm::openai::OpenAiProvider;
use crate::tools::ToolRegistry;

/// Run a single agent turn for `prompt` and stream the reply to stdout.
///
/// Without `--yes` the approval policy is `ReadOnly`, so mutating tools are
/// auto-denied and the agent never blocks on an interactive prompt; with
/// `--yes` it is `Auto`, running every tool. Either way the agent never parks
/// on the approvals channel, so draining events until `TurnComplete`/`Done`
/// terminates.
pub async fn handle(
    prompt: String,
    yes: bool,
    project_path: Option<&Path>,
    current_file: Option<&Path>,
    model_override: Option<&str>,
    base_url_override: Option<&str>,
) -> Result<()> {
    let mut cfg = Config::load()?;
    if let Some(m) = model_override {
        cfg.model = m.to_string();
    }
    if let Some(b) = base_url_override {
        cfg.base_url = b.trim_end_matches('/').to_string();
    }

    let provider: Arc<dyn Provider> = Arc::new(OpenAiProvider::from_config(&cfg));
    let registry = Arc::new(ToolRegistry::default_set());
    let ctx = PromptContext::build(project_path, current_file, &cfg.default_language);
    let cwd = project_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let history = vec![
        ChatMessage::system(ctx.system_prompt()),
        ChatMessage::user(prompt),
    ];

    let policy = if yes {
        ApprovalPolicy::Auto
    } else {
        ApprovalPolicy::ReadOnly
    };

    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<AgentEvent>();
    // ReadOnly/Auto never park on approval, so the receiver is only created to
    // satisfy the signature; nothing is ever sent on it.
    let (_appr_tx, appr_rx) = mpsc::unbounded_channel();

    // Drive the agent on a task so we can drain its events as they arrive.
    let agent_task = tokio::spawn(async move {
        agent::run_agent(
            provider, registry, history, ev_tx, appr_rx, &cfg, &cwd, policy,
        )
        .await
    });

    let mut stdout = std::io::stdout();
    let mut wrote_any = false;
    let mut error: Option<String> = None;
    while let Some(ev) = ev_rx.recv().await {
        match ev {
            AgentEvent::AssistantTextDelta(d) => {
                print!("{d}");
                let _ = stdout.flush();
                wrote_any = true;
            }
            AgentEvent::AssistantTextDone => {}
            AgentEvent::ToolCallRequested { name, args } => {
                eprintln!("→ {name}: {args}");
            }
            // Headless runs use ReadOnly/Auto, which never prompt — a denied
            // mutating tool arrives as ToolFinished{ok:false} below, not here.
            AgentEvent::ApprovalRequired { .. } => {}
            AgentEvent::ToolStarted { name } => {
                eprintln!("→ running {name}…");
            }
            AgentEvent::ToolFinished { name, output, ok } => {
                let tag = if ok { "ok" } else { "error" };
                eprintln!("→ {name} ({tag}): {output}");
            }
            // The agent reports failure via Error and then returns without a
            // Done/TurnComplete, so record it and stop draining.
            AgentEvent::Error(msg) => {
                eprintln!("error: {msg}");
                error = Some(msg);
                break;
            }
            AgentEvent::Done | AgentEvent::TurnComplete(_) => break,
        }
    }

    // Terminate the streamed line with a newline so the shell prompt is clean.
    if wrote_any {
        println!();
    }

    // The agent task completes once it emits TurnComplete; await it so the
    // process doesn't exit while it is still mid-flight.
    let _ = agent_task.await;

    // Propagate agent failure as a non-zero exit so `run` is scriptable.
    if let Some(msg) = error {
        bail!("agent run failed: {msg}");
    }
    Ok(())
}

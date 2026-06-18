use std::io::Write;
use std::path::Path;

use anyhow::{Result, bail};
use tokio::sync::mpsc;

use crate::agent::{self, AgentEvent, ApprovalPolicy};
use crate::config::Config;
use crate::llm::message::ChatMessage;

pub async fn handle(
    prompt: String,
    yes: bool,
    no_think: bool,
    project_path: Option<&Path>,
    current_file: Option<&Path>,
    model_override: Option<&str>,
    base_url_override: Option<&str>,
) -> Result<()> {
    let mut cfg = Config::load()?;
    cfg.apply_overrides(model_override, base_url_override);
    let session = crate::actions::build_session(&cfg, project_path, current_file);

    let history = vec![
        ChatMessage::system(session.ctx.system_prompt()),
        ChatMessage::user(prompt),
    ];

    let policy = if yes {
        ApprovalPolicy::Auto
    } else {
        ApprovalPolicy::ReadOnly
    };

    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<AgentEvent>();
    let (_appr_tx, appr_rx) = mpsc::unbounded_channel();

    let think = !no_think;
    let provider = session.provider;
    let registry = session.registry;
    let cwd = session.cwd;
    let agent_task = tokio::spawn(async move {
        agent::run_agent(
            provider, registry, history, ev_tx, appr_rx, &cfg, &cwd, policy, think,
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
            AgentEvent::ReasoningDelta(d) => {
                eprint!("{d}");
                let _ = std::io::stderr().flush();
            }
            AgentEvent::AssistantTextDone => {}
            AgentEvent::ToolCallRequested { name, args } => {
                eprintln!("→ {name}: {args}");
            }
            AgentEvent::ApprovalRequired { .. } => {}
            AgentEvent::ToolStarted { name } => {
                eprintln!("→ running {name}…");
            }
            AgentEvent::ToolFinished { name, output, ok } => {
                let tag = if ok { "ok" } else { "error" };
                eprintln!("→ {name} ({tag}): {output}");
            }
            AgentEvent::Error(msg) => {
                eprintln!("error: {msg}");
                error = Some(msg);
                break;
            }
            AgentEvent::Done | AgentEvent::TurnComplete(_) => break,
        }
    }

    if wrote_any {
        println!();
    }

    let _ = agent_task.await;

    if let Some(msg) = error {
        bail!("agent run failed: {msg}");
    }
    Ok(())
}

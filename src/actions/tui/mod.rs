use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

use crate::agent::{self, AgentEvent, ApprovalDecision, ApprovalPolicy};
use crate::config::Config;
use crate::llm::Provider;
use crate::llm::message::ChatMessage;
use crate::tools::ToolRegistry;

mod app;
mod banner;
mod render;

use app::{App, PendingApproval, Role};
use render::{draw, summarize_args};

const PAGE_SCROLL: usize = 10;

pub async fn handle(
    project_path: Option<&Path>,
    current_file: Option<&Path>,
    model_override: Option<&str>,
    base_url_override: Option<&str>,
) -> Result<()> {
    let mut cfg = Config::load()?;
    cfg.apply_overrides(model_override, base_url_override);
    let session = crate::actions::build_session(&cfg, project_path, current_file);

    let model = if cfg.model.is_empty() {
        "unknown".into()
    } else {
        cfg.model.clone()
    };
    let mut app = App::new(&cfg, model);
    app.history
        .push(ChatMessage::system(session.ctx.system_prompt()));

    let mut terminal = ratatui::init();
    let result = run(
        &mut terminal,
        &mut app,
        session.provider,
        session.registry,
        cfg,
        session.cwd,
    )
    .await;
    ratatui::restore();
    result
}

async fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    provider: Arc<dyn Provider>,
    registry: Arc<ToolRegistry>,
    cfg: Config,
    cwd: std::path::PathBuf,
) -> Result<()> {
    let (mut ev_tx, mut ev_rx) = mpsc::unbounded_channel::<AgentEvent>();

    loop {
        terminal.draw(|frame| draw(frame, app))?;
        while let Ok(ev) = ev_rx.try_recv() {
            match ev {
                AgentEvent::AssistantTextDelta(d) => {
                    app.reasoning.clear();
                    app.push_delta(&d);
                }
                AgentEvent::ReasoningDelta(d) => {
                    app.reasoning.push_str(&d);
                    app.scroll_from_bottom = 0;
                }
                AgentEvent::AssistantTextDone => app.end_assistant(),
                AgentEvent::ToolCallRequested { name, args } => {
                    app.reasoning.clear();
                    app.push(Role::Tool, format!("{name}: {}", summarize_args(&args)));
                }
                AgentEvent::ApprovalRequired { name, args } => {
                    let pending = PendingApproval { name, args };
                    app.push(
                        Role::System,
                        format!(
                            "approve {} {}? [y]es / [n]o / [a]lways",
                            pending.name,
                            summarize_args(&pending.args)
                        ),
                    );
                    app.pending_approval = Some(pending);
                }
                AgentEvent::ToolStarted { name } => {
                    app.push(Role::Tool, format!("running {name}…"));
                }
                AgentEvent::ToolFinished { name, output, ok } => {
                    let role = if ok { Role::Tool } else { Role::Error };
                    app.push(role, format!("{name}: {output}"));
                }
                AgentEvent::Error(msg) => {
                    app.thinking = false;
                    app.end_assistant();
                    app.reasoning.clear();
                    app.pending_approval = None;
                    app.push(Role::Error, msg);
                }
                AgentEvent::Done => {
                    app.thinking = false;
                    app.end_assistant();
                    app.reasoning.clear();
                }
                AgentEvent::TurnComplete(turns) => {
                    app.history.extend(turns);
                    app.thinking = false;
                    app.end_assistant();
                    app.reasoning.clear();
                    app.appr_tx = None;
                    app.pending_approval = None;
                    app.agent_task = None;
                }
            }
        }

        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if app.pending_approval.is_some() {
            if key.code == KeyCode::Esc {
                app.stop_turn();
                (ev_tx, ev_rx) = mpsc::unbounded_channel();
                continue;
            }
            if let Some(decision) = app.approval_key(key.code) {
                if let Some(tx) = &app.appr_tx {
                    let _ = tx.send(decision);
                }
                let verb = match decision {
                    ApprovalDecision::Approve => "approved",
                    ApprovalDecision::ApproveAlways => "approved (always)",
                    ApprovalDecision::Deny => "denied",
                };
                if let Some(pending) = app.pending_approval.take() {
                    app.push(Role::System, format!("{verb} {}", pending.name));
                }
            }
            continue; // swallow all keys while a prompt is pending
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Esc => {
                if app.thinking {
                    app.stop_turn();
                    (ev_tx, ev_rx) = mpsc::unbounded_channel();
                }
            }
            KeyCode::Enter => {
                if app.thinking || app.input.trim().is_empty() {
                    continue;
                }
                let text = std::mem::take(&mut app.input).trim().to_string();
                if text == "/clear" {
                    app.messages.clear();
                    let system = app.history.first().cloned();
                    app.history.clear();
                    if let Some(s) = system {
                        app.history.push(s);
                    }
                    app.push(Role::System, "Started a new conversation.".into());
                    continue;
                }
                app.push(Role::User, text.clone());
                app.history.push(ChatMessage::user(text));
                app.thinking = true;

                // Each turn gets its own approval channel. The sender lives in
                // `App` for the (Phase 6) approval UI; the receiver moves into
                // the spawned agent task.
                let (appr_tx, appr_rx) = mpsc::unbounded_channel::<ApprovalDecision>();
                app.appr_tx = Some(appr_tx);

                let provider = provider.clone();
                let registry = registry.clone();
                let history = app.history.clone();
                let ev_tx = ev_tx.clone();
                let policy = ApprovalPolicy::from_str(&cfg.approval_policy);
                let agent_cfg = cfg.clone();
                let cwd = cwd.clone();
                // Capture the thinking toggle for this request.
                let think = app.think;
                let handle = tokio::spawn(async move {
                    let new_turns = agent::run_agent(
                        provider,
                        registry,
                        history,
                        ev_tx.clone(),
                        appr_rx,
                        &agent_cfg,
                        &cwd,
                        policy,
                        think,
                    )
                    .await;
                    let _ = ev_tx.send(AgentEvent::TurnComplete(new_turns));
                });
                app.agent_task = Some(handle);
            }
            // Tab and Ctrl+I (which most terminals deliver as Tab) reveal the
            // endpoint in the status bar.
            KeyCode::Tab => app.show_details = !app.show_details,
            KeyCode::Char('i') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.show_details = !app.show_details;
            }
            // Ctrl+T toggles whether the next request lets the model think.
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.think = !app.think;
            }
            KeyCode::Backspace => {
                app.input.pop();
            }
            KeyCode::Up => app.scroll_from_bottom += 1,
            KeyCode::Down => app.scroll_from_bottom = app.scroll_from_bottom.saturating_sub(1),
            KeyCode::PageUp => app.scroll_from_bottom += PAGE_SCROLL,
            KeyCode::PageDown => {
                app.scroll_from_bottom = app.scroll_from_bottom.saturating_sub(PAGE_SCROLL)
            }
            KeyCode::Char(c) => app.input.push(c),
            _ => {}
        }
    }
    Ok(())
}

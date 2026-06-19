use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

use crate::agent::{self, AgentEvent, ApprovalDecision, ApprovalPolicy, QuestionAnswer};
use crate::config::Config;
use crate::context::PromptContext;
use crate::llm::Provider;
use crate::llm::message::ChatMessage;
use crate::tools::ToolRegistry;

mod app;
mod banner;
mod markdown;
mod render;

use app::{App, EntryKind, EntryState, PendingApproval, PendingQuestion, Role};
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
    app.history.push(crate::actions::system_message_for(
        &session.ctx,
        app.mode,
        &session.cwd,
    ));

    let mut terminal = ratatui::init();
    let result = run(
        &mut terminal,
        &mut app,
        session.provider,
        session.ctx,
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
    ctx: PromptContext,
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
                    app.push(Role::Tool, format!("{name}: {}", summarize_args(&name, &args)));
                }
                AgentEvent::ApprovalRequired { name, args } => {
                    let pending = PendingApproval { name, args };
                    app.push(
                        Role::Prompt,
                        format!(
                            "approve {} {}? [y]es / [n]o / [a]lways",
                            pending.name,
                            summarize_args(&pending.name, &pending.args)
                        ),
                    );
                    app.pending_approval = Some(pending);
                }
                AgentEvent::QuestionRaised { question, options } => {
                    app.pending_question = Some(PendingQuestion {
                        question,
                        options,
                        selected: 0,
                        entry: None,
                    });
                    app.scroll_from_bottom = 0;
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
                    app.pending_question = None;
                    app.q_tx = None;
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
                    app.pending_question = None;
                    app.q_tx = None;
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
        if let Some(pq) = app.pending_question.as_mut() {
            // `answer` is set when the question is resolved; the actual send/clear
            // happens after the `pq` borrow ends to avoid borrow conflicts on `app`.
            let mut answer: Option<String> = None;
            let mut dismiss = false;
            if let Some(entry) = pq.entry.as_mut() {
                match key.code {
                    KeyCode::Esc => {
                        pq.entry = None;
                    }
                    KeyCode::Enter => {
                        // Ignore an empty submission — keep editing rather than
                        // sending a blank answer or note to the agent.
                        if !entry.buffer.trim().is_empty() {
                            answer = Some(match &entry.kind {
                                EntryKind::FreeText => entry.buffer.clone(),
                                EntryKind::Note(i) => {
                                    app::note_answer(&pq.options[*i].label, &entry.buffer)
                                }
                            });
                        }
                    }
                    KeyCode::Backspace => {
                        entry.buffer.pop();
                    }
                    KeyCode::Char(c) => {
                        entry.buffer.push(c);
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Up => {
                        pq.selected = pq.selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        if !pq.other_selected() {
                            pq.selected += 1;
                        }
                    }
                    KeyCode::Char('n') => {
                        if !pq.other_selected() {
                            pq.entry = Some(EntryState {
                                kind: EntryKind::Note(pq.selected),
                                buffer: String::new(),
                            });
                        }
                    }
                    KeyCode::Enter => {
                        if pq.other_selected() {
                            pq.entry = Some(EntryState {
                                kind: EntryKind::FreeText,
                                buffer: String::new(),
                            });
                        } else {
                            answer = Some(pq.options[pq.selected].label.clone());
                        }
                    }
                    KeyCode::Esc => {
                        dismiss = true;
                    }
                    _ => {}
                }
            }
            if dismiss {
                // Dropping the only QuestionAnswer sender makes the agent's
                // questions.recv() return None; it substitutes the dismissal
                // sentinel as the tool result and finishes the turn normally,
                // so the in-flight history (and any tool results already
                // produced this turn) is preserved.
                app.q_tx = None;
                app.pending_question = None;
                app.push(Role::System, "Dismissed.".into());
                continue;
            }
            if let Some(answer) = answer {
                if let Some(tx) = &app.q_tx {
                    let _ = tx.send(QuestionAnswer(answer));
                }
                app.pending_question = None;
                app.push(Role::System, "Answered.".into());
            }
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
            continue;
        }
        if matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL) {
            if app.quit_armed {
                break;
            }
            app.quit_armed = true;
            continue;
        }
        app.quit_armed = false;
        match key.code {
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

                let (appr_tx, appr_rx) = mpsc::unbounded_channel::<ApprovalDecision>();
                app.appr_tx = Some(appr_tx);
                let (q_tx, q_rx) = mpsc::unbounded_channel::<QuestionAnswer>();
                app.q_tx = Some(q_tx);

                let provider = provider.clone();
                let registry = Arc::new(ToolRegistry::for_mode(app.mode));
                let history = app.history.clone();
                let ev_tx = ev_tx.clone();
                let policy = ApprovalPolicy::from_str(&cfg.approval_policy);
                let agent_cfg = cfg.clone();
                let cwd = cwd.clone();
                let think = app.think;
                let handle = tokio::spawn(async move {
                    let new_turns = agent::run_agent(
                        provider,
                        registry,
                        history,
                        ev_tx.clone(),
                        appr_rx,
                        q_rx,
                        &agent_cfg,
                        &cwd,
                        policy,
                        think,
                        true,
                    )
                    .await;
                    let _ = ev_tx.send(AgentEvent::TurnComplete(new_turns));
                });
                app.agent_task = Some(handle);
            }
            KeyCode::BackTab => {
                if !app.thinking {
                    app.mode = app.mode.next();
                    if let Some(first) = app.history.first_mut() {
                        *first = crate::actions::system_message_for(&ctx, app.mode, &cwd);
                    }
                    app.push(Role::System, format!("Switched to {} mode.", app.mode.label()));
                }
            }
            KeyCode::Tab => app.show_details = !app.show_details,
            KeyCode::Char('i') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.show_details = !app.show_details;
            }
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

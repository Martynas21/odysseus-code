use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph};
use tokio::sync::mpsc;

use crate::client::{ClientError, HistoryMessage, OdysseusClient};
use crate::config::Config;
use crate::context::PromptContext;
use crate::session::SessionStore;

const PAGE_SCROLL: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
    Error,
}

#[derive(Debug, Clone)]
struct DisplayMessage {
    role: Role,
    content: String,
}

struct App {
    session: String,
    endpoint: String,
    model: String,
    messages: Vec<DisplayMessage>,
    input: String,
    /// Scroll position measured in rows up from the bottom of the transcript.
    /// 0 means "stick to the latest message".
    scroll_from_bottom: usize,
    thinking: bool,
}

impl App {
    fn new(cfg: &Config, session: String, history: Vec<HistoryMessage>) -> Self {
        let messages = history
            .into_iter()
            .map(|m| {
                let role = if m.role == "user" {
                    Role::User
                } else {
                    Role::Assistant
                };
                DisplayMessage {
                    role,
                    content: strip_context_prefix(&m.content).to_string(),
                }
            })
            .collect();
        Self {
            session,
            endpoint: cfg.endpoint.clone(),
            model: if cfg.model.is_empty() {
                "(default)".into()
            } else {
                cfg.model.clone()
            },
            messages,
            input: String::new(),
            scroll_from_bottom: 0,
            thinking: false,
        }
    }

    fn push(&mut self, role: Role, content: String) {
        self.messages.push(DisplayMessage { role, content });
        // New content should be visible immediately.
        self.scroll_from_bottom = 0;
    }
}

pub async fn handle(
    session_id: Option<&str>,
    project_path: Option<&Path>,
    current_file: Option<&Path>,
) -> Result<()> {
    let cfg = Config::load()?;
    let client = OdysseusClient::from_config(&cfg)?;
    let mut store = SessionStore::load()?;

    let session = super::resolve_session(&client, &cfg, &mut store, session_id).await?;
    let history = client.history(&session).await?;
    let ctx = PromptContext::build(project_path, current_file, &cfg.default_language);

    let mut app = App::new(&cfg, session, history);
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, client, ctx).await;
    ratatui::restore();
    result
}

async fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    client: OdysseusClient,
    ctx: PromptContext,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Result<String, ClientError>>();

    loop {
        terminal.draw(|frame| draw(frame, app))?;

        // Replies arrive from the background send task; never block the draw loop.
        if let Ok(reply) = rx.try_recv() {
            app.thinking = false;
            match reply {
                Ok(text) => app.push(Role::Assistant, text),
                Err(err) => app.push(Role::Error, err.to_string()),
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
        match key.code {
            KeyCode::Esc => break,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Enter => {
                if app.thinking || app.input.trim().is_empty() {
                    continue;
                }
                let text = std::mem::take(&mut app.input);
                let text = text.trim().to_string();
                app.push(Role::User, text.clone());
                app.thinking = true;
                let client = client.clone();
                let session = app.session.clone();
                let message = ctx.wrap(&text);
                let tx = tx.clone();
                tokio::spawn(async move {
                    let _ = tx.send(client.chat(&session, &message).await);
                });
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

fn draw(frame: &mut Frame, app: &mut App) {
    let [msg_area, input_area, status_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    // Transcript pane. Text is pre-wrapped so the scroll offset is exact.
    let width = msg_area.width.saturating_sub(2).max(1) as usize;
    let viewport = msg_area.height.saturating_sub(2) as usize;
    let lines = message_lines(&app.messages, width, app.thinking);
    app.scroll_from_bottom = app
        .scroll_from_bottom
        .min(lines.len().saturating_sub(viewport));
    let offset = scroll_offset(lines.len(), viewport, app.scroll_from_bottom);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(Block::bordered().title("odysseus-code"))
            .scroll((offset, 0)),
        msg_area,
    );

    // Input line.
    frame.render_widget(
        Paragraph::new(app.input.as_str())
            .block(Block::bordered().title("prompt (Enter to send, Esc to quit)")),
        input_area,
    );
    let cursor_x = (app.input.chars().count() as u16).min(input_area.width.saturating_sub(2));
    frame.set_cursor_position((input_area.x + 1 + cursor_x, input_area.y + 1));

    // Status bar.
    frame.render_widget(
        Paragraph::new(status_line(app)).style(Style::new().fg(Color::Black).bg(Color::Gray)),
        status_area,
    );
}

fn status_line(app: &App) -> String {
    let mut status = format!(
        " {} | model: {} | session: {}",
        app.endpoint, app.model, app.session
    );
    if app.thinking {
        status.push_str(" | thinking…");
    }
    status
}

/// Strip the `[context] {…} [/context]` metadata block that
/// `PromptContext::wrap` prefixes to every user message before display.
fn strip_context_prefix(content: &str) -> &str {
    if let Some(rest) = content.strip_prefix("[context] ")
        && let Some(end) = rest.find(" [/context]")
    {
        return rest[end + " [/context]".len()..].trim_start();
    }
    content
}

/// Hard-wrap text at `width` characters, preserving existing line breaks.
/// Empty input still occupies one row.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for line in text.split('\n') {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            rows.push(String::new());
            continue;
        }
        for chunk in chars.chunks(width) {
            rows.push(chunk.iter().collect());
        }
    }
    rows
}

/// Render the transcript as styled lines: a label per message, its wrapped
/// content, and a trailing blank line, plus a "thinking…" tail while a reply
/// is pending.
fn message_lines(messages: &[DisplayMessage], width: usize, thinking: bool) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for message in messages {
        let (label, color) = match message.role {
            Role::User => ("You", Color::Cyan),
            Role::Assistant => ("Odysseus", Color::Green),
            Role::Error => ("Error", Color::Red),
        };
        lines.push(Line::from(Span::styled(
            format!("{label}:"),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        )));
        for row in wrap_text(&message.content, width) {
            lines.push(Line::from(row));
        }
        lines.push(Line::default());
    }
    if thinking {
        lines.push(Line::from(Span::styled(
            "thinking…",
            Style::new()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    }
    lines
}

/// Rows to skip from the top so that `from_bottom == 0` shows the newest
/// lines and larger values scroll back through history.
fn scroll_offset(total_rows: usize, viewport_rows: usize, from_bottom: usize) -> u16 {
    let max = total_rows.saturating_sub(viewport_rows);
    max.saturating_sub(from_bottom.min(max)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::PromptContext;

    #[test]
    fn strip_context_prefix_removes_wrapped_metadata() {
        let ctx = PromptContext::build(Some(Path::new("/proj")), None, "rust");
        let wrapped = ctx.wrap("Explain borrowing");
        assert_eq!(strip_context_prefix(&wrapped), "Explain borrowing");
    }

    #[test]
    fn strip_context_prefix_leaves_plain_messages_alone() {
        assert_eq!(strip_context_prefix("just a message"), "just a message");
        // Only a leading block is stripped.
        let middle = "hello [context] {} [/context] world";
        assert_eq!(strip_context_prefix(middle), middle);
    }

    #[test]
    fn strip_context_prefix_requires_closing_tag() {
        let broken = "[context] {\"language\":\"rust\"} no closer";
        assert_eq!(strip_context_prefix(broken), broken);
    }

    #[test]
    fn wrap_text_hard_wraps_and_preserves_blank_lines() {
        assert_eq!(wrap_text("abcdef", 4), vec!["abcd", "ef"]);
        assert_eq!(wrap_text("ab\n\ncd", 10), vec!["ab", "", "cd"]);
        assert_eq!(wrap_text("", 10), vec![""]);
    }

    #[test]
    fn wrap_text_counts_chars_not_bytes() {
        assert_eq!(wrap_text("héllö wörld", 6), vec!["héllö ", "wörld"]);
    }

    #[test]
    fn message_lines_labels_roles_and_appends_thinking() {
        let messages = vec![
            DisplayMessage {
                role: Role::User,
                content: "hi".into(),
            },
            DisplayMessage {
                role: Role::Assistant,
                content: "hello".into(),
            },
        ];
        let lines = message_lines(&messages, 80, true);
        // label + content + blank per message, plus the thinking tail
        assert_eq!(lines.len(), 7);
        assert_eq!(lines[0].to_string(), "You:");
        assert_eq!(lines[1].to_string(), "hi");
        assert_eq!(lines[3].to_string(), "Odysseus:");
        assert_eq!(lines[6].to_string(), "thinking…");

        let without = message_lines(&messages, 80, false);
        assert_eq!(without.len(), 6);
    }

    #[test]
    fn scroll_offset_sticks_to_bottom_and_clamps() {
        // 10 rows in a 4-row viewport: bottom shows rows 6..10 (offset 6).
        assert_eq!(scroll_offset(10, 4, 0), 6);
        assert_eq!(scroll_offset(10, 4, 2), 4);
        // Scrolling past the top clamps to 0.
        assert_eq!(scroll_offset(10, 4, 100), 0);
        // Content shorter than the viewport never scrolls.
        assert_eq!(scroll_offset(3, 4, 0), 0);
        assert_eq!(scroll_offset(3, 4, 5), 0);
    }

    #[test]
    fn app_push_resets_scroll_and_history_strips_context() {
        let cfg = Config::default();
        let history = vec![
            HistoryMessage {
                role: "user".into(),
                content: "[context] {\"language\":\"rust\"} [/context]\n\nhi".into(),
            },
            HistoryMessage {
                role: "assistant".into(),
                content: "hey".into(),
            },
        ];
        let mut app = App::new(&cfg, "s1".into(), history);
        assert_eq!(app.messages[0].content, "hi");
        assert_eq!(app.messages[0].role, Role::User);
        assert_eq!(app.messages[1].role, Role::Assistant);

        app.scroll_from_bottom = 7;
        app.push(Role::Assistant, "new".into());
        assert_eq!(app.scroll_from_bottom, 0);
    }

    #[test]
    fn status_line_mentions_thinking_only_while_pending() {
        let cfg = Config::default();
        let mut app = App::new(&cfg, "s1".into(), Vec::new());
        assert!(!status_line(&app).contains("thinking"));
        assert!(status_line(&app).contains("model: (default)"));
        assert!(status_line(&app).contains("session: s1"));
        app.thinking = true;
        assert!(status_line(&app).contains("thinking…"));
    }
}

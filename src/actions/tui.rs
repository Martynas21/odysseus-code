use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph};
use tokio::sync::mpsc;

use crate::client::{ClientError, HistoryMessage, OdysseusClient};
use crate::config::Config;
use crate::context::PromptContext;
use crate::session::{DEFAULT_SESSION_NAME, SessionStore};

const PAGE_SCROLL: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
    Error,
    /// A local note from the client itself (e.g. after `/clear`), shown
    /// dimmed and without a speaker label.
    System,
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
    /// When true, the status bar also shows the endpoint and session id.
    /// Toggled with Tab (Ctrl+I); off by default to keep the chrome minimal.
    show_details: bool,
    /// Start time, used to animate the background ship.
    started: Instant,
}

impl App {
    fn new(cfg: &Config, session: String, model: String, history: Vec<HistoryMessage>) -> Self {
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
            model,
            messages,
            input: String::new(),
            scroll_from_bottom: 0,
            thinking: false,
            show_details: false,
            started: Instant::now(),
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

    let (name, session) =
        super::resolve_session_named(&client, &cfg, &mut store, session_id).await?;
    let history = client.history(&session).await?;
    let model = resolve_model_name(&client, &cfg, &session).await;
    let ctx = PromptContext::build(project_path, current_file, &cfg.default_language);

    let mut app = App::new(&cfg, session, model, history);
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, client, ctx, cfg, store, name).await;
    ratatui::restore();
    result
}

/// Best-effort display name for the model backing `session_id`: the configured
/// model if set, otherwise whatever the server reports for that session, and
/// "unknown" if neither is available.
async fn resolve_model_name(client: &OdysseusClient, cfg: &Config, session_id: &str) -> String {
    if !cfg.model.is_empty() {
        return cfg.model.clone();
    }
    if let Ok(sessions) = client.list_sessions().await
        && let Some(info) = sessions.into_iter().find(|s| s.id == session_id)
        && !info.model.is_empty()
    {
        return info.model;
    }
    "unknown".into()
}

async fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    client: OdysseusClient,
    ctx: PromptContext,
    cfg: Config,
    mut store: SessionStore,
    session_name: Option<String>,
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
                if text == "/clear" {
                    // Reset inline: session creation is a quick call, unlike
                    // chat replies which stay on the background task.
                    if let Err(err) =
                        start_new_session(app, &client, &cfg, &mut store, session_name.as_deref())
                            .await
                    {
                        app.push(Role::Error, format!("could not start a new session: {err}"));
                    } else if session_name.is_some()
                        && let Err(err) = store.save()
                    {
                        app.push(
                            Role::Error,
                            format!("new session started but could not be saved: {err}"),
                        );
                    }
                    continue;
                }
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
            // Tab and Ctrl+I (which most terminals deliver as Tab) reveal the
            // endpoint and session id in the status bar.
            KeyCode::Tab => app.show_details = !app.show_details,
            KeyCode::Char('i') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.show_details = !app.show_details;
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

/// Replace the current conversation with a brand-new server session so the
/// model genuinely forgets prior context. On success the transcript is wiped
/// down to a single dimmed note, and the store mapping for `name` (if any) is
/// repointed at the fresh session so the next launch reuses it too.
///
/// The store is updated in memory only; the caller is responsible for saving.
async fn start_new_session(
    app: &mut App,
    client: &OdysseusClient,
    cfg: &Config,
    store: &mut SessionStore,
    name: Option<&str>,
) -> Result<()> {
    let session_name = name.unwrap_or(DEFAULT_SESSION_NAME);
    let info = super::create_session(client, cfg, session_name).await?;
    if let Some(name) = name {
        store.insert(name, &info.id);
    }
    app.session = info.id;
    if !info.model.is_empty() {
        app.model = info.model;
    }
    app.messages.clear();
    app.push(Role::System, "Started a new session.".into());
    Ok(())
}

fn draw(frame: &mut Frame, app: &mut App) {
    let [msg_area, input_area, status_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    // Background ship, drawn first so the transcript text overlays it.
    render_ship(frame, msg_area, app.started.elapsed().as_secs_f64());

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
            .block(Block::bordered().title("prompt")),
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
    // Collapsed: just the model. Tab/Ctrl+I expands the connection details.
    let mut status = format!(" {}", app.model);
    if app.show_details {
        status.push_str(&format!(" | {} | session: {}", app.endpoint, app.session));
    }
    if app.thinking {
        status.push_str(" | thinking…");
    }
    status
}

/// A detailed three-masted galleon, tall centre mast flanked by shorter fore
/// and aft masts of billowing sails, riding the sea at the foot of the pane.
/// `ODYSSEUS-CODE` is centred on the hull. Spaces are transparent; every other
/// glyph is painted dimmed.
const SHIP: &[&str] = &[
    "                    |>",
    "                    |)",
    "                    |_)",
    "      |)            |__)          |)",
    "      |_)           |___)         |_)",
    "      |__)          |____)        |__)",
    "      |___)         |_____)       |___)",
    "      |__)          |____)        |__)",
    "  ____|_____________|_____________|____",
    "  \\ o   o   o   o   o   o   o   o   o /",
    "   \\           ODYSSEUS-CODE         /",
    "    \\_______________________________/",
];

/// Number of scrolling wave rows that form the sea at the foot of the pane.
const SEA_ROWS: i32 = 4;

/// How many of the ship's bottom rows sit inside the sea, so the hull rides in
/// the water and the waves behind it stay visible through the gaps.
const HULL_SUBMERGE: i32 = 2;

/// Open-sea swell: full height, lively spatial frequency, drifting at the
/// natural wave speed. The higher frequency lets many crests ripple across the
/// water at once.
const SEA_AMPLITUDE: f64 = 1.2;
const SEA_SPEED: f64 = 1.0;
const SEA_FREQUENCY: f64 = 0.10;

/// Ship swell: a heavy hull has inertia, so it answers the water with a smaller
/// amplitude, a slower roll, and a *much* lower spatial frequency. Because cells
/// can only step by whole rows, a high frequency would shear the hull at every
/// wavefront; keeping the wavelength far wider than the ship means neighbouring
/// columns almost always share the same offset, so at most one gentle step ever
/// travels under the hull instead of a ripple of tears.
const SHIP_AMPLITUDE: f64 = 0.6;
const SHIP_SPEED: f64 = 0.5;
const SHIP_FREQUENCY: f64 = 0.035;

/// Per-column vertical displacement (in rows) of a swell at horizontal position
/// `x` and time `t`, scaled by `amplitude`, travelling leftward at `speed`, with
/// spatial `frequency` setting how tightly crests pack. Crests travel toward
/// smaller `x` so the ship reads as sailing forward (to the right). A low
/// frequency keeps neighbouring columns nearly level so the art shears by at
/// most one row.
fn column_wave_offset(x: i32, t: f64, amplitude: f64, speed: f64, frequency: f64) -> i32 {
    ((x as f64 * frequency + t * speed).sin() * amplitude).round() as i32
}

/// Draw the animated ship and sea inside `area`'s border. The sea is painted
/// first so it shows through the ship's gaps; the hull then rides a couple of
/// rows into it. Both the water and the ship are lifted by the same per-column
/// swell, so the ship rolls with the waves. Cells are only painted within the
/// bordered interior; `put` clips anything that falls outside.
fn render_ship(frame: &mut Frame, area: Rect, t: f64) {
    let inner = area.inner(Margin::new(1, 1));
    let ship_h = SHIP.len() as i32;
    let ship_w = SHIP.iter().map(|l| l.chars().count()).max().unwrap_or(0) as i32;
    // Skip rendering when the pane can't hold the ship riding in its sea.
    if (inner.width as i32) < ship_w || (inner.height as i32) < ship_h + SEA_ROWS - HULL_SUBMERGE {
        return;
    }

    let style = Style::new().fg(Color::Blue).add_modifier(Modifier::DIM);
    let buf = frame.buffer_mut();
    let sea_top = inner.bottom() as i32 - SEA_ROWS;

    // The ship: centred, riding HULL_SUBMERGE rows into the sea, and lifted by
    // its own gentler swell so it rolls with the water instead of bobbing as a
    // block.
    let base_x = inner.x as i32 + (inner.width as i32 - ship_w) / 2;
    let base_y = sea_top - ship_h + HULL_SUBMERGE;
    let ship_y =
        |x: i32, ry: i32| base_y + ry + column_wave_offset(x, t, SHIP_AMPLITUDE, SHIP_SPEED, SHIP_FREQUENCY);

    // The ship's opaque silhouette: every cell from a row's first to its last
    // glyph, keyed at the ship's own swell offset. Because the hull rolls more
    // gently than the water, sea and ship occupy different rows in the overlap
    // band; without this mask the waves would tear through the solid hull. We
    // skip the sea on these cells so the water only shows through true gaps
    // below and around the ship.
    let mut occluded: std::collections::HashSet<(i32, i32)> = std::collections::HashSet::new();
    for (ry, line) in SHIP.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let first = chars.iter().position(|&c| c != ' ');
        let last = chars.iter().rposition(|&c| c != ' ');
        if let (Some(first), Some(last)) = (first, last) {
            for cx in first..=last {
                let x = base_x + cx as i32;
                occluded.insert((x, ship_y(x, ry as i32)));
            }
        }
    }

    // The sea: crests drift sideways over time (a per-row phase shift layers
    // the rows) and the whole surface rides the swell. Cells behind the ship's
    // hull are left unpainted so the water never bleeds through it.
    for row in 0..SEA_ROWS {
        for sx in 0..inner.width as i32 {
            let x = inner.x as i32 + sx;
            let y = sea_top + row + column_wave_offset(x, t, SEA_AMPLITUDE, SEA_SPEED, SEA_FREQUENCY);
            if occluded.contains(&(x, y)) {
                continue;
            }
            let v = (sx as f64 * 0.30 + t * 1.2 + row as f64 * 0.8).sin();
            let ch = if v > 0.5 {
                '^'
            } else if v < -0.5 {
                '_'
            } else {
                '~'
            };
            put(buf, inner, x, y, ch, style);
        }
    }

    // The hull and rigging, drawn over the cleared silhouette.
    for (ry, line) in SHIP.iter().enumerate() {
        for (cx, ch) in line.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            let x = base_x + cx as i32;
            put(buf, inner, x, ship_y(x, ry as i32), ch, style);
        }
    }
}

/// Paint a single character if `(x, y)` lies within `bounds`. Out-of-range
/// positions (including negative coordinates from the drift) are ignored.
fn put(buf: &mut Buffer, bounds: Rect, x: i32, y: i32, ch: char, style: Style) {
    if x < bounds.left() as i32 || x >= bounds.right() as i32 {
        return;
    }
    if y < bounds.top() as i32 || y >= bounds.bottom() as i32 {
        return;
    }
    if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
        cell.set_char(ch);
        cell.set_style(style);
    }
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
        // System notes are unlabelled, dimmed italic asides.
        if message.role == Role::System {
            for row in wrap_text(&message.content, width) {
                lines.push(Line::from(Span::styled(
                    row,
                    Style::new()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )));
            }
            lines.push(Line::default());
            continue;
        }
        let (label, color) = match message.role {
            Role::User => ("You", Color::Cyan),
            Role::Assistant => ("Odysseus", Color::Green),
            Role::Error => ("Error", Color::Red),
            Role::System => unreachable!("handled above"),
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
        let mut app = App::new(&cfg, "s1".into(), "qwen3".into(), history);
        assert_eq!(app.messages[0].content, "hi");
        assert_eq!(app.messages[0].role, Role::User);
        assert_eq!(app.messages[1].role, Role::Assistant);

        app.scroll_from_bottom = 7;
        app.push(Role::Assistant, "new".into());
        assert_eq!(app.scroll_from_bottom, 0);
    }

    #[test]
    fn column_wave_offset_is_strict_and_ship_rides_softer() {
        let mut sea_moves = 0;
        let mut ship_moves = 0;
        // Adjacent-column offset changes are the visible "tears" in the art; the
        // ship's gentler frequency must keep these far rarer than the sea's.
        let mut sea_tears = 0;
        let mut ship_tears = 0;
        for step in 0..100 {
            let t = step as f64 * 0.1;
            let mut prev_sea = None;
            let mut prev_ship = None;
            for x in 0..400 {
                let sea = column_wave_offset(x, t, SEA_AMPLITUDE, SEA_SPEED, SEA_FREQUENCY);
                let ship = column_wave_offset(x, t, SHIP_AMPLITUDE, SHIP_SPEED, SHIP_FREQUENCY);
                // Both are clamped to a single row so nothing shears by more than one.
                assert!((-1..=1).contains(&sea), "sea {sea} out of range");
                assert!((-1..=1).contains(&ship), "ship {ship} out of range");
                sea_moves += i32::from(sea != 0);
                ship_moves += i32::from(ship != 0);
                sea_tears += i32::from(prev_sea.is_some_and(|p| p != sea));
                ship_tears += i32::from(prev_ship.is_some_and(|p| p != ship));
                prev_sea = Some(sea);
                prev_ship = Some(ship);
            }
        }
        // The hull heaves on far fewer columns than the water around it.
        assert!(ship_moves < sea_moves, "ship {ship_moves} >= sea {sea_moves}");
        // And it shears between neighbours far less often, so it reads as smooth.
        assert!(ship_tears * 2 < sea_tears, "ship tears {ship_tears} vs sea {sea_tears}");
        // The swell travels: the surface differs as time advances.
        let a: Vec<i32> =
            (0..40).map(|x| column_wave_offset(x, 0.0, SEA_AMPLITUDE, SEA_SPEED, SEA_FREQUENCY)).collect();
        let b: Vec<i32> =
            (0..40).map(|x| column_wave_offset(x, 1.5, SEA_AMPLITUDE, SEA_SPEED, SEA_FREQUENCY)).collect();
        assert_ne!(a, b);
    }

    #[test]
    fn waves_never_tear_through_the_hull() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        // Across a full swell cycle the ship's opaque silhouette must hold: no
        // sea crest (`~`/`^`, which the hull art never uses) may appear between
        // a hull row's left and right edge. Before the occlusion mask the waves
        // sheared straight through the solid hull because it rolled on a gentler
        // offset than the water.
        let area = Rect::new(0, 0, 46, 20);
        for step in 0..40 {
            let t = step as f64 * 0.2;
            let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
            terminal.draw(|f| render_ship(f, area, t)).unwrap();
            let buf = terminal.backend().buffer();

            let inner = area.inner(Margin::new(1, 1));
            let ship_w = SHIP.iter().map(|l| l.chars().count()).max().unwrap() as i32;
            let base_x = inner.x as i32 + (inner.width as i32 - ship_w) / 2;
            let base_y = inner.bottom() as i32 - SEA_ROWS - SHIP.len() as i32 + HULL_SUBMERGE;

            for (ry, line) in SHIP.iter().enumerate() {
                let chars: Vec<char> = line.chars().collect();
                let (Some(first), Some(last)) = (
                    chars.iter().position(|&c| c != ' '),
                    chars.iter().rposition(|&c| c != ' '),
                ) else {
                    continue;
                };
                for cx in first..=last {
                    let x = base_x + cx as i32;
                    let y = base_y + ry as i32
                        + column_wave_offset(x, t, SHIP_AMPLITUDE, SHIP_SPEED, SHIP_FREQUENCY);
                    if x < inner.left() as i32
                        || x >= inner.right() as i32
                        || y < inner.top() as i32
                        || y >= inner.bottom() as i32
                    {
                        continue;
                    }
                    let sym = buf[(x as u16, y as u16)].symbol();
                    assert!(
                        sym != "~" && sym != "^",
                        "wave {sym:?} inside hull at ({x},{y}) t={t}"
                    );
                }
            }
        }
    }

    #[test]
    fn put_ignores_out_of_bounds_without_panicking() {
        use ratatui::layout::Position;
        let bounds = Rect::new(2, 2, 4, 4);
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 10));
        let style = Style::new();
        // Negative y, and coordinates outside the bounds, are all no-ops.
        put(&mut buf, bounds, 3, -1, '#', style);
        put(&mut buf, bounds, 0, 3, '#', style);
        put(&mut buf, bounds, 3, 3, '#', style);
        assert_eq!(buf[Position::new(3, 3)].symbol(), "#");
        assert_eq!(buf[Position::new(0, 3)].symbol(), " ");
    }

    #[test]
    fn message_lines_renders_system_note_without_label() {
        let messages = vec![DisplayMessage {
            role: Role::System,
            content: "Started a new session.".into(),
        }];
        let lines = message_lines(&messages, 80, false);
        // System notes have no "Label:" prefix — just the dimmed content.
        assert!(lines.iter().all(|l| l.to_string() != "System:"));
        assert_eq!(lines[0].to_string(), "Started a new session.");
    }

    #[tokio::test]
    async fn start_new_session_resets_transcript_and_remaps_store() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/session")
            .with_status(200)
            .with_body(r#"{"id":"new-sid","name":"odysseus-code","model":"qwen3"}"#)
            .create_async()
            .await;

        let cfg = Config {
            endpoint: server.url(),
            endpoint_id: "ep1".into(),
            model: "qwen3".into(),
            ..Config::default()
        };
        let client = crate::client::OdysseusClient::new(server.url(), "ody_tok");
        let mut store = SessionStore::default();
        store.insert("odysseus-code", "old-sid");

        let history = vec![HistoryMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let mut app = App::new(&cfg, "old-sid".into(), "old-model".into(), history);
        assert_eq!(app.messages.len(), 1);

        start_new_session(&mut app, &client, &cfg, &mut store, Some("odysseus-code"))
            .await
            .unwrap();

        // New server session swapped in, transcript replaced by the note only.
        assert_eq!(app.session, "new-sid");
        // The status bar tracks the new session's model.
        assert_eq!(app.model, "qwen3");
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].role, Role::System);
        assert_eq!(app.scroll_from_bottom, 0);
        // Store now points the friendly name at the fresh session.
        assert_eq!(store.server_id("odysseus-code"), Some("new-sid"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn start_new_session_without_name_leaves_store_untouched() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/session")
            .with_status(200)
            .with_body(r#"{"id":"new-sid","name":"odysseus-code","model":"qwen3"}"#)
            .create_async()
            .await;

        let cfg = Config {
            endpoint: server.url(),
            endpoint_id: "ep1".into(),
            model: "qwen3".into(),
            ..Config::default()
        };
        let client = crate::client::OdysseusClient::new(server.url(), "ody_tok");
        let mut store = SessionStore::default();
        let mut app = App::new(&cfg, "raw-id".into(), "qwen3".into(), Vec::new());

        start_new_session(&mut app, &client, &cfg, &mut store, None)
            .await
            .unwrap();

        assert_eq!(app.session, "new-sid");
        // A raw-id launch has no friendly name to remap.
        assert_eq!(store, SessionStore::default());
    }

    #[test]
    fn status_line_is_just_the_model_by_default() {
        let cfg = Config::default();
        let mut app = App::new(&cfg, "s1".into(), "qwen3".into(), Vec::new());
        let line = status_line(&app);
        assert!(line.contains("qwen3"));
        // No labels, endpoint, or session id while collapsed.
        assert!(!line.contains("model:"));
        assert!(!line.contains("session"));
        assert!(!line.contains("localhost"));
        assert!(!line.contains("thinking"));
        app.thinking = true;
        assert!(status_line(&app).contains("thinking…"));
    }

    #[test]
    fn status_line_expands_endpoint_and_session_when_details_on() {
        let cfg = Config::default();
        let mut app = App::new(&cfg, "sess-123".into(), "qwen3".into(), Vec::new());
        app.show_details = true;
        let line = status_line(&app);
        assert!(line.contains("qwen3"));
        assert!(line.contains("http://localhost:7000"));
        assert!(line.contains("session: sess-123"));
    }

    #[tokio::test]
    async fn resolve_model_name_prefers_configured_model() {
        let cfg = Config {
            model: "configured-model".into(),
            ..Config::default()
        };
        // The client points nowhere: a configured model short-circuits any call.
        let client = crate::client::OdysseusClient::new("http://127.0.0.1:1", "tok");
        assert_eq!(
            resolve_model_name(&client, &cfg, "any").await,
            "configured-model"
        );
    }

    #[tokio::test]
    async fn resolve_model_name_falls_back_to_server_session_model() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/sessions")
            .with_status(200)
            .with_body(r#"[{"id":"sess-123","name":"x","model":"server-model"}]"#)
            .create_async()
            .await;
        let cfg = Config::default(); // no configured model
        let client = crate::client::OdysseusClient::new(server.url(), "tok");
        assert_eq!(
            resolve_model_name(&client, &cfg, "sess-123").await,
            "server-model"
        );
    }

    #[tokio::test]
    async fn resolve_model_name_is_unknown_when_unresolvable() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/sessions")
            .with_status(200)
            .with_body("[]")
            .create_async()
            .await;
        let cfg = Config::default();
        let client = crate::client::OdysseusClient::new(server.url(), "tok");
        assert_eq!(resolve_model_name(&client, &cfg, "missing").await, "unknown");
    }
}

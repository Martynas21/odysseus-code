use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
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
    /// Start time, used for the steady, mode-independent bird wing-beat.
    started: Instant,
    /// Accumulated drift phase for the scrolling sky and waves. Advanced each
    /// frame by the elapsed time, faster while `thinking`, so entering thinking
    /// mode accelerates the scene from where it is instead of jumping.
    anim_phase: f64,
    /// Wall-clock instant of the previous frame, used to measure that elapsed
    /// time.
    last_tick: Instant,
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
            endpoint: cfg.base_url.clone(),
            model,
            messages,
            input: String::new(),
            scroll_from_bottom: 0,
            thinking: false,
            show_details: false,
            started: Instant::now(),
            anim_phase: 0.0,
            last_tick: Instant::now(),
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
    // TODO(3.2): becomes PromptContext::system_prompt seed; unused until TUI rewrite.
    _ctx: PromptContext,
    cfg: Config,
    mut store: SessionStore,
    session_name: Option<String>,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<ChatUpdate>();

    loop {
        terminal.draw(|frame| draw(frame, app))?;
        while let Ok(update) = rx.try_recv() {
            match update {
                ChatUpdate::Notice(note) => app.push(Role::System, note),
                ChatUpdate::Reply(text) => {
                    app.thinking = false;
                    app.push(Role::Assistant, text);
                }
                ChatUpdate::Failed(msg) => {
                    app.thinking = false;
                    app.push(Role::Error, msg);
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
                // TODO(3.2): PromptContext::system_prompt replaces wrap(); TUI rewritten in Task 3.2.
                let message = text.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    let notify = tx.clone();
                    let result = chat_with_retries(&client, &session, &message, |note| {
                        let _ = notify.send(ChatUpdate::Notice(note));
                    })
                    .await;
                    let _ = tx.send(match result {
                        Ok(text) => ChatUpdate::Reply(text),
                        Err(msg) => ChatUpdate::Failed(msg),
                    });
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

/// A message from the background chat task to the draw loop. Retries stream
/// `Notice`s ahead of the single terminal `Reply` or `Failed`.
enum ChatUpdate {
    Notice(String),
    Reply(String),
    Failed(String),
}

/// Initial attempt plus this many retries before giving up.
const MAX_RETRIES: u32 = 5;

/// Whether a chat failure is worth retrying. Transient problems (network, rate
/// limiting, malformed bodies, server-side 5xx) are; permanent ones (auth, and
/// other 4xx client errors) are not — retrying them only delays the real error.
fn is_retryable(err: &ClientError) -> bool {
    match err {
        ClientError::Network { .. } | ClientError::RateLimited | ClientError::BadResponse(_) => {
            true
        }
        ClientError::Http { status, .. } => *status >= 500,
        ClientError::Unauthorized => false,
    }
}

/// Send a chat message, retrying empty replies and transient failures up to
/// [`MAX_RETRIES`] times. `notice` is called with a human-readable progress
/// line before each retry. Returns the first non-empty reply, or an error
/// string once retries are exhausted or a permanent failure occurs.
async fn chat_with_retries(
    client: &OdysseusClient,
    session: &str,
    message: &str,
    mut notice: impl FnMut(String),
) -> Result<String, String> {
    for retry in 0..=MAX_RETRIES {
        let outcome = client.chat(session, message).await;
        match &outcome {
            // A non-empty reply is the only success.
            Ok(text) if !text.trim().is_empty() => return Ok(text.clone()),
            // A permanent failure won't fix itself — surface it now.
            Err(err) if !is_retryable(err) => return Err(err.to_string()),
            _ => {}
        }
        let reason = match &outcome {
            Ok(_) => "the model returned an empty reply".to_string(),
            Err(err) => err.to_string(),
        };
        if retry == MAX_RETRIES {
            return Err(format!("{reason} (gave up after {MAX_RETRIES} retries)"));
        }
        notice(format!(
            "{reason} — retrying ({}/{MAX_RETRIES})…",
            retry + 1
        ));
    }
    unreachable!("loop returns on the final attempt")
}

fn draw(frame: &mut Frame, app: &mut App) {
    let [msg_area, ship_area, input_area, status_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(BOAT_BAND_HEIGHT),
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

    // Boat banner: its own band between transcript and prompt, so the
    // conversation can never collide with it. The scene drifts constantly and
    // speeds up while a reply is pending, accelerating smoothly from its current
    // phase rather than resetting.
    let now = Instant::now();
    let dt = now.saturating_duration_since(app.last_tick).as_secs_f64();
    app.last_tick = now;
    app.anim_phase = anim_step(app.anim_phase, dt, app.thinking);
    render_banner(
        frame,
        ship_area,
        app.anim_phase,
        app.started.elapsed().as_secs_f64(),
    );

    // Input line.
    frame.render_widget(
        Paragraph::new(app.input.as_str()).block(Block::bordered().title("prompt")),
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

/// A small triangle-sailed dinghy: two rows of sail above a hull that rides the
/// waterline. Spaces are transparent; every other glyph is painted dimmed.
const BOAT: &[&str] = &[" |\\", " |_\\", "\\____/"];

/// Height of the boat band, in rows: one sky row for the clouds and birds, the
/// boat's two sail rows, and the rippling waterline its hull sits on.
const BOAT_BAND_HEIGHT: u16 = 4;

/// Clouds drift along the top of the band. The small one is a single row; the
/// big one is two rows, so its underside spills onto the first sail row — and,
/// drawn in front of the boat, hides the sail there. Spaces are transparent.
const CLOUD_SMALL: &[&str] = &[".-~-."];
const CLOUD_BIG: &[&str] = &[".-~-.", "(___)"];

/// Birds flit through the two sail rows behind the boat (the sails paint over,
/// hiding, any that overlap). Each flaps by alternating a down-stroke `v` and an
/// up-stroke `^` at `BIRD_FLAP_RATE` beats per second.
const BIRD_DOWN: char = 'v';
const BIRD_UP: char = '^';
const BIRD_FLAP_RATE: f64 = 5.0;

/// Crest drift of the waterline, in radians of phase per unit drift. A higher
/// cadence packs the crests so the water reads as moving.
const WAVE_CADENCE: f64 = 1.2;

/// Sky drift speeds, in columns per unit drift. Parallax: clouds (far) crawl,
/// birds (nearer) are quicker, and both trail the water.
const CLOUD_SPEED: f64 = 1.5;
const BIRD_SPEED: f64 = 2.5;

/// How much faster the whole scene drifts while a reply is pending. The drift
/// phase is accumulated (see `anim_step`), so this only changes the *rate* — the
/// scene accelerates from its current position rather than jumping.
const THINK_SPEEDUP: f64 = 4.0;

/// Advance the accumulated drift phase by one frame's `dt` seconds, scaled up
/// while `thinking`. `dt` is clamped so a delayed frame can't lurch the scene.
fn anim_step(phase: f64, dt: f64, thinking: bool) -> f64 {
    let rate = if thinking { THINK_SPEEDUP } else { 1.0 };
    phase + dt.clamp(0.0, 0.25) * rate
}

/// Glyph of the swell at column `x` and time `t`: `^` crest, `_` trough, `~`
/// between. `cadence` sets how fast crests drift past a fixed column, so a
/// higher cadence makes the water rush by.
fn wave_glyph(x: i32, t: f64, cadence: f64) -> char {
    let v = (x as f64 * 0.30 + t * cadence).sin();
    if v > 0.5 {
        '^'
    } else if v < -0.5 {
        '_'
    } else {
        '~'
    }
}

/// A flapping bird's wing at time `t`, offset by `phase` so a flock doesn't beat
/// in unison: the down-stroke `v` and up-stroke `^` alternate at `BIRD_FLAP_RATE`.
fn bird_glyph(t: f64, phase: f64) -> char {
    if ((t * BIRD_FLAP_RATE + phase) as i64).rem_euclid(2) == 0 {
        BIRD_DOWN
    } else {
        BIRD_UP
    }
}

/// Left-edge column of a sprite that scrolls leftward and wraps. `phase` is its
/// start offset in columns, `t` the elapsed time, `speed` its drift in columns
/// per second, `span` the wrap width (band width + sprite width) and `sprite_w`
/// the sprite's width. The result runs from `-sprite_w` (just off the left) up
/// to `span - sprite_w` (just off the right), so the sprite re-enters smoothly.
fn scroll_x(phase: f64, t: f64, speed: f64, span: i32, sprite_w: i32) -> i32 {
    (phase - t * speed).rem_euclid(span as f64) as i32 - sprite_w
}

/// Width (in display columns) of the widest row of a sprite.
fn sprite_width(sprite: &[&str]) -> i32 {
    sprite.iter().map(|l| l.chars().count()).max().unwrap_or(0) as i32
}

/// Paint a multi-row sprite at `(x, y)`, treating spaces as transparent. Later
/// `put_sprite` calls overpaint earlier ones, so draw order sets the z-order.
fn put_sprite(buf: &mut Buffer, area: Rect, sprite: &[&str], x: i32, y: i32, style: Style) {
    for (ry, line) in sprite.iter().enumerate() {
        for (cx, ch) in line.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            put(buf, area, x + cx as i32, y + ry as i32, ch, style);
        }
    }
}

/// Draw the sky, boat, and rippling waterline into `area` (no border; the band
/// spans the full width). Layered back to front so the z-order reads right:
/// birds (behind the sails) → water → boat → clouds (in front of the sails).
/// The boat stays parked at centre; the water, clouds, and birds scroll by
/// `drift` (accumulated upstream, faster while thinking) so the boat reads as
/// speeding along without moving. Wings flap on the real-time `flap_t` so they
/// stay a steady beat regardless of drift speed. `put` clips outside `area`.
fn render_banner(frame: &mut Frame, area: Rect, drift: f64, flap_t: f64) {
    let boat_w = sprite_width(BOAT);
    let boat_h = BOAT.len() as i32;
    // Nothing sensible to draw if the band can't hold the boat and a sky row.
    if (area.height as i32) <= boat_h || (area.width as i32) < boat_w {
        return;
    }

    let style = Style::new().fg(Color::Blue).add_modifier(Modifier::DIM);

    let width = area.width as i32;
    let sky_top = area.y as i32;
    let water_y = area.bottom() as i32 - 1;
    let buf = frame.buffer_mut();

    // Birds first, scattered across the two sail rows, so the boat's sails paint
    // over (hide) any that overlap them.
    let bird_w = 1;
    let bird_span = width + bird_w;
    for (i, &(frac, row)) in [(0.10, 1), (0.38, 2), (0.63, 1), (0.86, 2)]
        .iter()
        .enumerate()
    {
        let x = scroll_x(
            frac * bird_span as f64,
            drift,
            BIRD_SPEED,
            bird_span,
            bird_w,
        );
        put(
            buf,
            area,
            x,
            sky_top + row,
            bird_glyph(flap_t, i as f64),
            style,
        );
    }

    // The waterline: the band's bottom row, rippling across its full width.
    for sx in 0..width {
        let x = area.x as i32 + sx;
        put(
            buf,
            area,
            x,
            water_y,
            wave_glyph(x, drift, WAVE_CADENCE),
            style,
        );
    }

    // The boat, hull on the waterline and sails above, parked at centre and
    // drawn over the birds and swell so neither shows through the solid hull.
    let base_x = area.x as i32 + (width - boat_w) / 2;
    let base_y = water_y - (boat_h - 1);
    put_sprite(buf, area, BOAT, base_x, base_y, style);

    // Clouds last, along the top row, so a big cloud's underside hides the first
    // sail row where it overlaps.
    let cloud_w = sprite_width(CLOUD_BIG);
    let cloud_span = width + cloud_w;
    for &(frac, sprite) in &[(0.00, CLOUD_BIG), (0.45, CLOUD_SMALL), (0.72, CLOUD_BIG)] {
        let x = scroll_x(
            frac * cloud_span as f64,
            drift,
            CLOUD_SPEED,
            cloud_span,
            cloud_w,
        );
        put_sprite(buf, area, sprite, x, sky_top, style);
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

    #[test]
    fn strip_context_prefix_removes_wrapped_metadata() {
        // TODO(3.2): legacy wrap() format; strip_context_prefix removed in Task 3.2.
        let wrapped = "[context] {\"project_path\":\"/proj\"} [/context]\n\nExplain borrowing";
        assert_eq!(strip_context_prefix(wrapped), "Explain borrowing");
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
    fn wave_glyph_uses_only_swell_chars_and_ripples() {
        // The waterline is built from exactly these three glyphs.
        for x in 0..200 {
            for step in 0..50 {
                let g = wave_glyph(x, step as f64 * 0.1, WAVE_CADENCE);
                assert!(matches!(g, '^' | '~' | '_'), "unexpected wave glyph {g:?}");
            }
        }
        // It ripples on its own: the row differs as the drift advances, so the
        // water moves even while the boat sits still.
        let a: String = (0..40).map(|x| wave_glyph(x, 0.0, WAVE_CADENCE)).collect();
        let b: String = (0..40).map(|x| wave_glyph(x, 1.5, WAVE_CADENCE)).collect();
        assert_ne!(a, b, "waterline did not move over time");
    }

    #[test]
    fn anim_step_accelerates_smoothly_without_resetting() {
        // Thinking advances the drift faster than idle over the same frame.
        let dt = 0.05;
        assert!(
            anim_step(0.0, dt, true) > anim_step(0.0, dt, false),
            "thinking did not speed the drift up"
        );
        // Crucially, flipping into thinking continues from the current phase — a
        // small forward step, never a jump — even after a long idle run. This is
        // the whole point: speed up from where we are, don't reset.
        let phase = 123.4; // a large accumulated phase, as after minutes idle
        let next = anim_step(phase, dt, true);
        assert!(
            next > phase && next - phase < 1.0,
            "drift jumped on entering thinking: {phase} -> {next}"
        );
        // A delayed frame can't lurch the scene: dt is clamped.
        assert_eq!(
            anim_step(0.0, 10.0, false),
            anim_step(0.0, 0.25, false),
            "dt was not clamped"
        );
    }

    /// Render the band at drift/flap time `t` into rows of plain text.
    #[cfg(test)]
    fn banner_rows(area: Rect, t: f64) -> Vec<String> {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut term = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        term.draw(|f| render_banner(f, area, t, t)).unwrap();
        let buf = term.backend().buffer();
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn boat_holds_position_while_water_races() {
        let area = Rect::new(0, 0, 40, BOAT_BAND_HEIGHT);
        let last = area.height as usize - 1;
        // The hull's `\`…`/` are the only such glyphs the band ever draws, so
        // their columns pin the boat's position.
        let hull_marks = |rows: &[String]| -> Vec<usize> {
            rows[last]
                .char_indices()
                .filter(|&(_, c)| c == '\\' || c == '/')
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        };

        let early = banner_rows(area, 0.5);
        let later = banner_rows(area, 1.5);
        // As the drift advances the hull occupies the same columns — the boat
        // only ever sits at centre.
        assert_eq!(
            hull_marks(&early),
            hull_marks(&later),
            "boat shifted instead of staying put"
        );
        // But the waterline differs, because the swell has drifted on.
        assert_ne!(
            early[last], later[last],
            "waterline did not move with the drift"
        );
    }

    #[test]
    fn render_banner_draws_boat_on_a_rippling_waterline() {
        let area = Rect::new(0, 0, 40, BOAT_BAND_HEIGHT);
        let rows = banner_rows(area, 0.0);

        // A mast stands among the sail rows, the hull rides the bottom waterline,
        // and open water (swell glyphs) flanks the hull on that same row.
        assert!(
            rows.iter().any(|r| r.contains('|')),
            "no mast/sail anywhere: {rows:?}"
        );
        let waterline = &rows[area.height as usize - 1];
        assert!(
            waterline.contains('\\') && waterline.contains('/'),
            "no hull on the waterline: {waterline:?}"
        );
        assert!(
            waterline.chars().any(|c| matches!(c, '^' | '~' | '_')),
            "no open water beside the hull: {waterline:?}"
        );
    }

    #[test]
    fn sky_drifts_left_and_wraps() {
        let span = 45;
        let w = 5;
        // From a mid-band phase (clear of the wrap seam) the sprite slides left
        // as the drift advances.
        let phase = 20.0;
        assert!(
            scroll_x(phase, 0.0, CLOUD_SPEED, span, w) > scroll_x(phase, 1.0, CLOUD_SPEED, span, w),
            "cloud did not drift left over time"
        );
        // It never strays outside its off-screen-either-side travel range.
        for step in 0..200 {
            let x = scroll_x(0.0, step as f64 * 0.1, CLOUD_SPEED, span, w);
            assert!((-w..span - w).contains(&x), "cloud {x} left its range");
        }
    }

    #[test]
    fn birds_flap_between_wing_strokes() {
        // Across time a bird shows both the down- and up-stroke, and nothing else.
        let seen: std::collections::HashSet<char> =
            (0..40).map(|s| bird_glyph(s as f64 * 0.1, 0.0)).collect();
        assert!(
            seen.contains(&'v') && seen.contains(&'^'),
            "bird never flapped: {seen:?}"
        );
        assert!(
            seen.iter().all(|c| matches!(c, 'v' | '^')),
            "stray glyph: {seen:?}"
        );
    }

    #[test]
    fn sails_block_birds_but_clouds_block_sails() {
        let area = Rect::new(0, 0, 40, BOAT_BAND_HEIGHT);
        let boat_w = sprite_width(BOAT);
        let base_x = (area.width as i32 - boat_w) / 2;
        let sky_top = area.y as i32;
        // Solid sail glyphs: mast/spar on the first sail row, mast/spar on the
        // second. (Coordinates mirror the BOAT sprite.)
        let sail_cells = [
            (base_x + 1, sky_top + 1), // '|'
            (base_x + 2, sky_top + 1), // '\'
            (base_x + 1, sky_top + 2), // '|'
            (base_x + 3, sky_top + 2), // '\'
        ];

        let mut a_cloud_hid_a_sail = false;
        for step in 0..400 {
            let t = step as f64 * 0.1;
            let rows = banner_rows(area, t);
            for &(x, y) in &sail_cells {
                let ch = rows[y as usize].chars().nth(x as usize).unwrap();
                // Birds fly behind the sails, so neither wing-stroke (`v`/`^`) is
                // ever painted onto a sail.
                assert!(
                    ch != 'v' && ch != '^',
                    "bird showed through the sail at ({x},{y}) t={t}"
                );
                // Clouds are drawn in front, so a big one can replace a sail glyph
                // on the first sail row.
                if y == sky_top + 1 && matches!(ch, '.' | '-' | '~' | '(' | ')' | '_') {
                    a_cloud_hid_a_sail = true;
                }
            }
        }
        assert!(a_cloud_hid_a_sail, "clouds never covered a sail");
    }

    #[test]
    fn boat_banner_stays_out_of_the_transcript() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        // The boat lives in its own band between the transcript and the prompt,
        // so the conversation can never collide with it — the old overlap that
        // tinted transcript glyphs blue is now structurally impossible. Fill the
        // transcript with a wall of text and confirm every blue boat/water cell
        // sits inside the band, never up among the messages.
        let cfg = Config::default();
        let history: Vec<HistoryMessage> = (0..60)
            .map(|_| HistoryMessage {
                role: "assistant".into(),
                content: "X".repeat(80),
            })
            .collect();
        let mut app = App::new(&cfg, "s".into(), "m".into(), history);

        let area = Rect::new(0, 0, 60, 30);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer();

        // The three fixed-height regions sit at the foot, so the boat band is the
        // BOAT_BAND_HEIGHT rows above the prompt (3) and status (1).
        let band_bottom = area.height - 1 - 3;
        let band_top = band_bottom - BOAT_BAND_HEIGHT;

        let mut blue_cells = 0;
        for y in 0..area.height {
            for x in 0..area.width {
                if buf[(x, y)].fg == Color::Blue {
                    blue_cells += 1;
                    assert!(
                        (band_top..band_bottom).contains(&y),
                        "boat glyph leaked outside its band at ({x},{y})"
                    );
                }
            }
        }
        assert!(blue_cells > 0, "boat band drew nothing");
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
        server
            .mock("GET", "/api/models")
            .with_status(200)
            .with_body(
                r#"{"hosts":[],"items":[{"host":"custom","port":0,
                     "url":"http://x/v1/chat/completions",
                     "models":["qwen3"],"models_extra":[],
                     "endpoint_id":"ep1","endpoint_name":"local llama"}]}"#,
            )
            .create_async()
            .await;
        let mock = server
            .mock("POST", "/api/session")
            .with_status(200)
            .with_body(r#"{"id":"new-sid","name":"odysseus-code","model":"qwen3"}"#)
            .create_async()
            .await;

        let cfg = Config {
            base_url: server.url(),
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
            .mock("GET", "/api/models")
            .with_status(200)
            .with_body(
                r#"{"hosts":[],"items":[{"host":"custom","port":0,
                     "url":"http://x/v1/chat/completions",
                     "models":["qwen3"],"models_extra":[],
                     "endpoint_id":"ep1","endpoint_name":"local llama"}]}"#,
            )
            .create_async()
            .await;
        server
            .mock("POST", "/api/session")
            .with_status(200)
            .with_body(r#"{"id":"new-sid","name":"odysseus-code","model":"qwen3"}"#)
            .create_async()
            .await;

        let cfg = Config {
            base_url: server.url(),
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
        assert_eq!(
            resolve_model_name(&client, &cfg, "missing").await,
            "unknown"
        );
    }

    #[tokio::test]
    async fn chat_with_retries_retries_empty_reply_then_succeeds() {
        let mut server = mockito::Server::new_async().await;
        let empty = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_body(r#"{"response":"  "}"#)
            .expect(1)
            .create_async()
            .await;
        let ok = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_body(r#"{"response":"hello"}"#)
            .expect(1)
            .create_async()
            .await;
        let client = crate::client::OdysseusClient::new(server.url(), "tok");

        let mut notices = Vec::new();
        let reply = chat_with_retries(&client, "s1", "hi", |n| notices.push(n)).await;

        assert_eq!(reply, Ok("hello".to_string()));
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].contains("empty") && notices[0].contains("(1/5)"),
            "unexpected notice: {:?}",
            notices[0]
        );
        empty.assert_async().await;
        ok.assert_async().await;
    }

    #[tokio::test]
    async fn chat_with_retries_retries_transient_server_error() {
        let mut server = mockito::Server::new_async().await;
        let boom = server
            .mock("POST", "/api/chat")
            .with_status(500)
            .with_body("boom")
            .expect(1)
            .create_async()
            .await;
        let ok = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_body(r#"{"response":"ok"}"#)
            .expect(1)
            .create_async()
            .await;
        let client = crate::client::OdysseusClient::new(server.url(), "tok");

        let mut notices = Vec::new();
        let reply = chat_with_retries(&client, "s1", "hi", |n| notices.push(n)).await;

        assert_eq!(reply, Ok("ok".to_string()));
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].contains("500") && notices[0].contains("(1/5)"),
            "unexpected notice: {:?}",
            notices[0]
        );
        boom.assert_async().await;
        ok.assert_async().await;
    }

    #[tokio::test]
    async fn chat_with_retries_does_not_retry_permanent_failure() {
        let mut server = mockito::Server::new_async().await;
        // expect(1): a 401 must be tried exactly once, never retried.
        let unauthorized = server
            .mock("POST", "/api/chat")
            .with_status(401)
            .with_body(r#"{"error":"Not authenticated"}"#)
            .expect(1)
            .create_async()
            .await;
        let client = crate::client::OdysseusClient::new(server.url(), "tok");

        let mut notices = Vec::new();
        let reply = chat_with_retries(&client, "s1", "hi", |n| notices.push(n)).await;

        assert!(reply.is_err());
        assert!(notices.is_empty(), "permanent failure should not retry");
        unauthorized.assert_async().await;
    }

    #[tokio::test]
    async fn chat_with_retries_gives_up_after_max_retries() {
        let mut server = mockito::Server::new_async().await;
        // Initial attempt plus five retries: six identical empty replies.
        let empty = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_body(r#"{"response":""}"#)
            .expect(6)
            .create_async()
            .await;
        let client = crate::client::OdysseusClient::new(server.url(), "tok");

        let mut notices = Vec::new();
        let reply = chat_with_retries(&client, "s1", "hi", |n| notices.push(n)).await;

        let err = reply.unwrap_err();
        assert!(
            err.contains("gave up after 5 retries"),
            "unexpected error: {err:?}"
        );
        assert_eq!(notices.len(), 5);
        empty.assert_async().await;
    }
}

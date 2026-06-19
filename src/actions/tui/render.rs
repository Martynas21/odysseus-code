use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph};

use super::app::{App, DisplayMessage, Role};
use super::banner::{BOAT_BAND_HEIGHT, anim_step, render_banner};

pub(super) fn summarize_args(args: &str) -> String {
    let flat: String = args
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    if flat.chars().count() > 80 {
        format!("{}…", flat.chars().take(80).collect::<String>())
    } else {
        flat
    }
}

fn reasoning_lines(reasoning: &str, width: usize) -> Vec<Line<'static>> {
    const REASONING_TAIL_ROWS: usize = 12;
    let style = Style::new()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::ITALIC);
    let mut lines = vec![Line::from(Span::styled("thinking…", style))];
    let rows = wrap_text(reasoning, width);
    let start = rows.len().saturating_sub(REASONING_TAIL_ROWS);
    for row in &rows[start..] {
        lines.push(Line::from(Span::styled(row.clone(), style)));
    }
    lines
}

pub(super) fn draw(frame: &mut Frame, app: &mut App) {
    let [msg_area, ship_area, input_area, status_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(BOAT_BAND_HEIGHT),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let width = msg_area.width.saturating_sub(2).max(1) as usize;
    let viewport = msg_area.height.saturating_sub(2) as usize;
    let mut lines = app.transcript_lines(width);
    if !app.reasoning.is_empty() {
        lines.extend(reasoning_lines(&app.reasoning, width));
    }
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

    frame.render_widget(
        Paragraph::new(app.input.as_str()).block(Block::bordered().title("prompt")),
        input_area,
    );
    let cursor_x = (app.input.chars().count() as u16).min(input_area.width.saturating_sub(2));
    frame.set_cursor_position((input_area.x + 1 + cursor_x, input_area.y + 1));

    let (status_text, status_style) = if app.quit_armed {
        (
            " Press Ctrl+C again to quit".to_string(),
            Style::new()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            status_line(app),
            Style::new().fg(Color::Black).bg(Color::Gray),
        )
    };
    frame.render_widget(Paragraph::new(status_text).style(status_style), status_area);
}

fn status_line(app: &App) -> String {
    let check = if app.think { 'x' } else { ' ' };
    let mut status = format!(" {}  [{check}] think", app.model);
    if app.show_details {
        status.push_str(&format!(" | {}", app.endpoint));
    }
    status
}

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

pub(super) fn message_lines(messages: &[DisplayMessage], width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for message in messages {
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
        if message.role == Role::Prompt {
            for row in wrap_text(&message.content, width) {
                lines.push(Line::from(Span::styled(
                    row,
                    Style::new()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            lines.push(Line::default());
            continue;
        }
        if message.role == Role::Tool {
            for (i, row) in wrap_text(&message.content, width).into_iter().enumerate() {
                let (text, style) = if i == 0 {
                    (
                        format!("→ {row}"),
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )
                } else {
                    (row, Style::new().fg(Color::DarkGray))
                };
                lines.push(Line::from(Span::styled(text, style)));
            }
            lines.push(Line::default());
            continue;
        }
        let (label, color) = match message.role {
            Role::User => ("You", Color::Cyan),
            Role::Assistant => ("Odysseus", Color::Green),
            Role::Error => ("Error", Color::Red),
            Role::Tool => unreachable!("handled above"),
            Role::System => unreachable!("handled above"),
            Role::Prompt => unreachable!("handled above"),
        };
        lines.push(Line::from(Span::styled(
            format!("{label}:"),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        )));
        if message.role == Role::Assistant {
            lines.extend(super::markdown::render(&message.content, width));
        } else {
            for row in wrap_text(&message.content, width) {
                lines.push(Line::from(row));
            }
        }
        lines.push(Line::default());
    }
    lines
}

fn scroll_offset(total_rows: usize, viewport_rows: usize, from_bottom: usize) -> u16 {
    let max = total_rows.saturating_sub(viewport_rows);
    max.saturating_sub(from_bottom.min(max)) as u16
}

#[cfg(test)]
mod tests;

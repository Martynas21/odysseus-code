use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph};

use super::app::{App, DisplayMessage, PendingQuestion, Role};
use super::banner::{BOAT_BAND_HEIGHT, anim_step, render_banner};

use super::markdown::first_heading;
use crate::tools::truncate_chars;

pub(super) fn summarize_args(name: &str, args: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(args) {
        let str_field = |key: &str| value.get(key).and_then(|v| v.as_str());
        let summary = match name {
            "write_file" => str_field("path").map(|path| {
                let mut s = path.to_string();
                if let Some(heading) = str_field("content").and_then(first_heading) {
                    s.push_str(&format!(" — \"{heading}\""));
                }
                s
            }),
            "edit_file" | "read_file" | "list_dir" => str_field("path").map(|p| p.to_string()),
            "grep" => str_field("pattern").map(|pattern| match str_field("path") {
                Some(path) => format!("{pattern} in {path}"),
                None => pattern.to_string(),
            }),
            "ask_user" => str_field("question").map(|q| q.to_string()),
            _ => None,
        };
        if let Some(summary) = summary {
            return truncate_chars(&summary, 80, "…");
        }
    }
    let flat: String = args
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    truncate_chars(&flat, 80, "…")
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

fn question_lines(pq: &PendingQuestion, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let sel_style = Style::new()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::new().fg(Color::DarkGray);

    for row in wrap_text(&pq.question, width) {
        lines.push(Line::from(Span::styled(
            row,
            Style::new().add_modifier(Modifier::BOLD),
        )));
    }

    let entry_style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let row_style = |selected: bool| if selected { sel_style } else { Style::new() };
    let marker = |selected: bool| if selected { "> " } else { "  " };

    for (i, opt) in pq.options.iter().enumerate() {
        let selected = pq.selected == i;
        let letter = (b'A' + (i % 26) as u8) as char;
        let mut spans = vec![Span::styled(
            format!("{}{letter}. {}", marker(selected), opt.label),
            row_style(selected),
        )];
        if let Some(buffer) = pq.note_buffer(i) {
            // Editing a note for this option, in place on its row.
            spans.push(Span::styled(format!("  note: {buffer}▌"), entry_style));
        } else if let Some(desc) = &opt.description {
            spans.push(Span::styled(format!("  {desc}"), dim_style));
        }
        lines.push(Line::from(spans));
    }

    let other_selected = pq.other_selected();
    let other = if let Some(buffer) = pq.free_text_buffer() {
        // Typing a free-text answer, edited in place on the Other row.
        Line::from(vec![
            Span::styled(format!("{}Other: ", marker(other_selected)), sel_style),
            Span::styled(format!("{buffer}▌"), entry_style),
        ])
    } else {
        Line::from(Span::styled(
            format!("{}Other… (type your own)", marker(other_selected)),
            row_style(other_selected),
        ))
    };
    lines.push(other);

    let hint = if pq.entry.is_some() {
        "Enter to submit · Esc to go back"
    } else {
        "↑/↓ move · Enter select · n add note · Esc cancel"
    };
    lines.push(Line::from(Span::styled(hint, dim_style)));

    lines.push(Line::default());
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
    if let Some(pq) = &app.pending_question {
        lines.extend(question_lines(pq, width));
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
    let mut status = format!(
        " {}  mode: {} (Shift+Tab)  [{check}] think",
        app.model,
        app.mode.label()
    );
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

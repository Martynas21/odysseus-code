use super::*;
use crate::config::Config;
use ratatui::layout::Rect;

#[test]
fn tool_line_renders_with_arrow_label() {
    let messages = vec![DisplayMessage {
        role: Role::Tool,
        content: "shell: ls -la".into(),
    }];
    let lines = message_lines(&messages, 80);
    assert!(
        lines
            .iter()
            .any(|l| l.to_string().contains("→ shell: ls -la"))
    );
}

#[test]
fn message_lines_styles_tool_and_error_distinctly() {
    let messages = vec![
        DisplayMessage {
            role: Role::Tool,
            content: "shell: echo hi".into(),
        },
        DisplayMessage {
            role: Role::Error,
            content: "denied shell".into(),
        },
    ];
    let lines = message_lines(&messages, 80);
    let tool_line = lines
        .iter()
        .find(|l| l.to_string().contains("echo hi"))
        .unwrap();
    assert!(tool_line.to_string().starts_with('→'));
    assert!(
        tool_line
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Yellow)),
        "tool call line should be yellow"
    );
    assert!(lines.iter().any(|l| l.to_string() == "Error:"));
    let error_label = lines.iter().find(|l| l.to_string() == "Error:").unwrap();
    assert!(
        error_label
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Red)),
        "error label should be red"
    );
}

#[test]
fn summarize_args_truncates_on_char_boundary() {
    let args = "é".repeat(200);
    let out = summarize_args(&args);
    assert!(out.ends_with('…'));
    assert_eq!(out.chars().count(), 81);
    assert_eq!(summarize_args("{\"a\":1}"), "{\"a\":1}");
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
fn message_lines_labels_roles_without_busy_text() {
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
    let lines = message_lines(&messages, 80);
    assert_eq!(lines.len(), 6);
    assert_eq!(lines[0].to_string(), "You:");
    assert_eq!(lines[1].to_string(), "hi");
    assert_eq!(lines[3].to_string(), "Odysseus:");
    assert!(lines.iter().all(|l| l.to_string() != "thinking…"));
}

#[test]
fn scroll_offset_sticks_to_bottom_and_clamps() {
    assert_eq!(scroll_offset(10, 4, 0), 6);
    assert_eq!(scroll_offset(10, 4, 2), 4);
    assert_eq!(scroll_offset(10, 4, 100), 0);
    assert_eq!(scroll_offset(3, 4, 0), 0);
    assert_eq!(scroll_offset(3, 4, 5), 0);
}

#[test]
fn boat_banner_stays_out_of_the_transcript() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let cfg = Config::default();
    let mut app = App::new(&cfg, "m".into());
    for _ in 0..60 {
        app.push(Role::Assistant, "X".repeat(80));
    }

    let area = Rect::new(0, 0, 60, 30);
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let buf = terminal.backend().buffer();

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
fn message_lines_renders_system_note_without_label() {
    let messages = vec![DisplayMessage {
        role: Role::System,
        content: "Started a new session.".into(),
    }];
    let lines = message_lines(&messages, 80);
    assert!(lines.iter().all(|l| l.to_string() != "System:"));
    assert_eq!(lines[0].to_string(), "Started a new session.");
}

#[test]
fn message_lines_highlights_approval_prompt() {
    let messages = vec![DisplayMessage {
        role: Role::Prompt,
        content: "approve shell ls? [y]es / [n]o / [a]lways".into(),
    }];
    let lines = message_lines(&messages, 80);
    let prompt = lines
        .iter()
        .find(|l| l.to_string().contains("approve shell"))
        .unwrap();
    let span = prompt.spans.first().unwrap();
    assert_eq!(span.style.fg, Some(Color::Black));
    assert_eq!(span.style.bg, Some(Color::Yellow));
    assert!(span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn status_line_shows_model_and_think_checkbox_by_default() {
    let cfg = Config::default();
    let mut app = App::new(&cfg, "qwen3".into());
    let line = status_line(&app);
    assert!(line.contains("qwen3"));
    assert!(!line.contains("model:"));
    assert!(!line.contains("session"));
    assert!(!line.contains("localhost"));
    assert!(!line.contains("thinking…"));
    app.thinking = true;
    assert!(!status_line(&app).contains("thinking…"));
}

#[test]
fn quit_armed_shows_confirmation_in_status_bar() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let cfg = Config::default();
    let mut app = App::new(&cfg, "qwen3".into());
    app.quit_armed = true;

    let area = Rect::new(0, 0, 60, 10);
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let buf = terminal.backend().buffer();

    let bottom = area.height - 1;
    let row: String = (0..area.width).map(|x| buf[(x, bottom)].symbol()).collect();
    assert!(row.contains("Press Ctrl+C again to quit"), "got: {row:?}");
    assert_eq!(buf[(1, bottom)].bg, Color::Yellow);
}

#[test]
fn status_line_think_checkbox_reflects_toggle() {
    let cfg = Config::default();
    let mut app = App::new(&cfg, "qwen3".into());
    assert!(status_line(&app).contains("[ ] think"));
    app.think = true;
    assert!(status_line(&app).contains("[x] think"));
}

#[test]
fn reasoning_lines_render_capped_thinking_block() {
    let reasoning: String = (0..50)
        .map(|i| format!("thought line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let lines = reasoning_lines(&reasoning, 80);
    assert_eq!(lines[0].to_string(), "thinking…");
    assert!(lines.len() <= 13);
    assert!(
        lines
            .last()
            .unwrap()
            .to_string()
            .contains("thought line 49")
    );
}

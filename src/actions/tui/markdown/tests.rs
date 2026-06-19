use super::*;
use ratatui::style::{Color, Modifier};

fn plain(lines: &[Line<'static>]) -> String {
    lines
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn span_with<'a>(
    lines: &'a [Line<'static>],
    needle: &str,
) -> &'a ratatui::text::Span<'static> {
    lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| s.content.contains(needle))
        .unwrap_or_else(|| panic!("no span containing {needle:?} in {:?}", lines))
}

#[test]
fn bold_renders_bold_without_markers() {
    let lines = render("hello **world**", 80);
    let text = plain(&lines);
    assert!(text.contains("hello world"), "got: {text:?}");
    assert!(!text.contains('*'), "markers should be gone: {text:?}");
    assert!(
        span_with(&lines, "world")
            .style
            .add_modifier
            .contains(Modifier::BOLD)
    );
}

#[test]
fn italic_renders_italic_without_markers() {
    let lines = render("an *emphatic* word", 80);
    assert!(!plain(&lines).contains('*'));
    assert!(
        span_with(&lines, "emphatic")
            .style
            .add_modifier
            .contains(Modifier::ITALIC)
    );
}

#[test]
fn inline_code_is_colored_without_backticks() {
    let lines = render("call `render()` now", 80);
    assert!(!plain(&lines).contains('`'));
    assert_eq!(span_with(&lines, "render()").style.fg, Some(Color::Cyan));
}

#[test]
fn heading_is_bold_without_hashes() {
    let lines = render("## Section Title", 80);
    let text = plain(&lines);
    assert!(text.contains("Section Title"), "got: {text:?}");
    assert!(!text.contains('#'), "hashes should be gone: {text:?}");
    assert!(
        span_with(&lines, "Section")
            .style
            .add_modifier
            .contains(Modifier::BOLD)
    );
}

#[test]
fn unordered_list_uses_bullet_prefix() {
    let lines = render("- first\n- second", 80);
    let text = plain(&lines);
    assert!(text.contains("• first"), "got: {text:?}");
    assert!(text.contains("• second"), "got: {text:?}");
}

#[test]
fn ordered_list_numbers_items() {
    let lines = render("1. alpha\n2. beta", 80);
    let text = plain(&lines);
    assert!(text.contains("1. alpha"), "got: {text:?}");
    assert!(text.contains("2. beta"), "got: {text:?}");
}

#[test]
fn nested_list_is_indented() {
    let lines = render("- outer\n  - inner", 80);
    let inner = lines
        .iter()
        .map(|l| l.to_string())
        .find(|s| s.contains("inner"))
        .unwrap();
    let outer = lines
        .iter()
        .map(|l| l.to_string())
        .find(|s| s.contains("outer"))
        .unwrap();
    let indent = |s: &str| s.len() - s.trim_start().len();
    assert!(
        indent(&inner) > indent(&outer),
        "inner {inner:?} should be more indented than outer {outer:?}"
    );
}

#[test]
fn blockquote_has_prefix() {
    let lines = render("> quoted text", 80);
    let text = plain(&lines);
    assert!(text.contains("quoted text"), "got: {text:?}");
    assert!(text.contains('▎'), "got: {text:?}");
}

#[test]
fn horizontal_rule_fills_width() {
    let lines = render("a\n\n---\n\nb", 20);
    let rule = lines
        .iter()
        .map(|l| l.to_string())
        .find(|s| s.contains('─'))
        .expect("a rule line");
    assert!(rule.chars().filter(|&c| c == '─').count() >= 10, "got: {rule:?}");
}

#[test]
fn code_block_highlights_keywords() {
    let lines = render("```rust\nfn main() {}\n```", 80);
    let text = plain(&lines);
    assert!(text.contains("fn main()"), "got: {text:?}");
    let kw = span_with(&lines, "fn");
    assert!(kw.style.fg.is_some(), "keyword should be colored: {kw:?}");
}

#[test]
fn code_block_lines_fill_width() {
    let lines = render("```\nhello\n```", 20);
    let code = lines
        .iter()
        .find(|l| l.to_string().contains("hello"))
        .expect("a code line");
    assert_eq!(code.to_string().chars().count(), 20, "should be full-width");
}

#[test]
fn code_block_strips_fences() {
    let text = plain(&render("```python\nx = 1\n```", 80));
    assert!(!text.contains("```"), "fences should be gone: {text:?}");
    assert!(text.contains("x = 1"), "got: {text:?}");
}

#[test]
fn table_renders_aligned_columns() {
    let md = "| Name | Age |\n| ---- | --- |\n| Bob | 30 |\n| Alicia | 7 |";
    let lines = render(md, 80);
    let text = plain(&lines);
    assert!(text.contains("Name"), "got: {text:?}");
    assert!(text.contains("Alicia"), "got: {text:?}");
    assert!(text.contains('│'), "needs column separators: {text:?}");
    assert!(text.contains('─'), "needs a header rule: {text:?}");
    let col = |needle: &str| {
        lines
            .iter()
            .map(|l| l.to_string())
            .find(|s| s.contains(needle))
            .and_then(|s| s.find(needle))
            .unwrap()
    };
    assert_eq!(col("Name"), col("Bob"));
    assert_eq!(col("Bob"), col("Alicia"));
}

#[test]
fn link_shows_text_and_url() {
    let lines = render("see [docs](https://example.com)", 80);
    let text = plain(&lines);
    assert!(text.contains("docs"), "got: {text:?}");
    assert!(text.contains("https://example.com"), "got: {text:?}");
}

#[test]
fn paragraph_wraps_within_width_preserving_style() {
    let lines = render("one two three four five **six** seven eight nine", 12);
    assert!(lines.len() > 1, "should wrap onto multiple lines");
    for line in &lines {
        assert!(
            line.to_string().chars().count() <= 12,
            "line over width: {:?}",
            line.to_string()
        );
    }
    assert!(
        span_with(&lines, "six")
            .style
            .add_modifier
            .contains(Modifier::BOLD),
        "bold must survive wrapping"
    );
}

#[test]
fn unterminated_emphasis_does_not_panic() {
    let lines = render("text with **unclosed bold", 80);
    assert!(plain(&lines).contains("unclosed bold"));
}

#[test]
fn unterminated_code_fence_does_not_panic() {
    let lines = render("```rust\nfn x() {", 80);
    assert!(plain(&lines).contains("fn x()"));
}

#[test]
fn empty_content_renders_nothing() {
    assert!(render("", 80).is_empty());
}

#[test]
fn tiny_width_does_not_panic() {
    let md = "# Title\n\nsome **bold** words\n\n- item one\n\n```rust\nfn main() {}\n```\n\n| a | b |\n| - | - |\n| 1 | 2 |";
    for width in [1, 2] {
        let _ = render(md, width);
    }
}

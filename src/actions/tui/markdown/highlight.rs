use std::sync::OnceLock;

use ratatui::style::{Color, Style};
use regex::Regex;

use super::Token;

const KEYWORD: Color = Color::Magenta;
const STRING: Color = Color::Green;
const NUMBER: Color = Color::Yellow;
const COMMENT: Color = Color::DarkGray;

#[derive(Clone, Copy, PartialEq)]
enum Comment {
    Slash,
    Hash,
    None,
}

struct Profile {
    keywords: &'static [&'static str],
    comment: Comment,
}

pub(super) fn highlight(lang: &str, line: &str) -> Vec<Token> {
    let profile = match profile(lang) {
        Some(p) => p,
        None => return vec![(Style::new(), line.to_string())],
    };
    let re = regex(profile.comment);
    let mut out: Vec<Token> = Vec::new();
    let mut last = 0;
    for caps in re.captures_iter(line) {
        let m = caps.get(0).unwrap();
        if m.start() > last {
            out.push((Style::new(), line[last..m.start()].to_string()));
        }
        let text = m.as_str().to_string();
        let style = if caps.name("comment").is_some() {
            Style::new().fg(COMMENT)
        } else if caps.name("str").is_some() {
            Style::new().fg(STRING)
        } else if caps.name("num").is_some() {
            Style::new().fg(NUMBER)
        } else if profile.keywords.contains(&text.as_str()) {
            Style::new().fg(KEYWORD)
        } else {
            Style::new()
        };
        out.push((style, text));
        last = m.end();
    }
    if last < line.len() {
        out.push((Style::new(), line[last..].to_string()));
    }
    if out.is_empty() {
        out.push((Style::new(), String::new()));
    }
    out
}

fn regex(comment: Comment) -> &'static Regex {
    static SLASH: OnceLock<Regex> = OnceLock::new();
    static HASH: OnceLock<Regex> = OnceLock::new();
    static NONE: OnceLock<Regex> = OnceLock::new();
    let comment_pat = match comment {
        Comment::Slash => r"(?P<comment>//[^\n]*)|",
        Comment::Hash => r"(?P<comment>#[^\n]*)|",
        Comment::None => "",
    };
    let build = || {
        let pat = format!(
            r#"{comment_pat}(?P<str>"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')|(?P<num>\b\d[\w.]*)|(?P<ident>[A-Za-z_]\w*)"#
        );
        Regex::new(&pat).expect("valid highlight regex")
    };
    match comment {
        Comment::Slash => SLASH.get_or_init(build),
        Comment::Hash => HASH.get_or_init(build),
        Comment::None => NONE.get_or_init(build),
    }
}

fn profile(lang: &str) -> Option<Profile> {
    let canon = match lang.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => "rust",
        "python" | "py" => "python",
        "js" | "javascript" | "ts" | "typescript" | "jsx" | "tsx" => "js",
        "bash" | "sh" | "shell" | "zsh" => "bash",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        _ => return None,
    };
    Some(match canon {
        "rust" => Profile {
            keywords: &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
                "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "trait", "true",
                "false", "type", "unsafe", "use", "where", "while",
            ],
            comment: Comment::Slash,
        },
        "python" => Profile {
            keywords: &[
                "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
                "del", "elif", "else", "except", "finally", "for", "from", "global", "if",
                "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise",
                "return", "True", "False", "try", "while", "with", "yield",
            ],
            comment: Comment::Hash,
        },
        "js" => Profile {
            keywords: &[
                "async",
                "await",
                "break",
                "case",
                "catch",
                "class",
                "const",
                "continue",
                "default",
                "delete",
                "do",
                "else",
                "export",
                "extends",
                "finally",
                "for",
                "function",
                "if",
                "import",
                "in",
                "instanceof",
                "let",
                "new",
                "null",
                "of",
                "return",
                "super",
                "switch",
                "this",
                "throw",
                "true",
                "false",
                "try",
                "typeof",
                "var",
                "void",
                "while",
                "yield",
            ],
            comment: Comment::Slash,
        },
        "bash" => Profile {
            keywords: &[
                "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "case",
                "esac", "function", "return", "local", "export", "echo",
            ],
            comment: Comment::Hash,
        },
        "json" => Profile {
            keywords: &["true", "false", "null"],
            comment: Comment::None,
        },
        "yaml" => Profile {
            keywords: &["true", "false", "null", "yes", "no"],
            comment: Comment::Hash,
        },
        "toml" => Profile {
            keywords: &["true", "false"],
            comment: Comment::Hash,
        },
        _ => unreachable!(),
    })
}

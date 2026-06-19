use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

mod highlight;

const CODE_BG: Color = Color::Rgb(40, 44, 52);

pub(super) fn render(content: &str, width: usize) -> Vec<Line<'static>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(content, opts);
    let mut r = Renderer::new(width);
    for event in parser {
        r.event(event);
    }
    r.finish()
}

type Token = (Style, String);

struct ListState {
    next: Option<u64>,
}

#[derive(Default)]
struct TableBuilder {
    rows: Vec<Vec<String>>,
    current: Vec<String>,
    cell: String,
}

struct Renderer {
    width: usize,
    out: Vec<Line<'static>>,
    inline: Vec<Token>,
    indent: Vec<String>,
    pending_first: Option<String>,
    lists: Vec<ListState>,
    quote_depth: usize,
    heading: Option<u8>,
    bold: u32,
    italic: u32,
    strike: u32,
    link: u32,
    urls: Vec<String>,
    code: Option<(String, String)>,
    table: Option<TableBuilder>,
}

impl Renderer {
    fn new(width: usize) -> Self {
        Self {
            width: width.max(1),
            out: Vec::new(),
            inline: Vec::new(),
            indent: Vec::new(),
            pending_first: None,
            lists: Vec::new(),
            quote_depth: 0,
            heading: None,
            bold: 0,
            italic: 0,
            strike: 0,
            link: 0,
            urls: Vec::new(),
            code: None,
            table: None,
        }
    }

    fn cur_indent(&self) -> String {
        self.indent.last().cloned().unwrap_or_default()
    }

    fn current_style(&self) -> Style {
        let mut style = Style::new();
        if self.bold > 0 || self.heading.is_some() {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.italic > 0 {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.strike > 0 {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.link > 0 {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        if let Some(level) = self.heading {
            style = style.fg(heading_color(level));
        }
        style
    }

    fn push_text(&mut self, text: &str, extra: Style) {
        let style = self.current_style().patch(extra);
        self.inline.push((style, text.to_string()));
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(Tag::Emphasis) => self.italic += 1,
            Event::End(TagEnd::Emphasis) => self.italic = self.italic.saturating_sub(1),
            Event::Start(Tag::Strong) => self.bold += 1,
            Event::End(TagEnd::Strong) => self.bold = self.bold.saturating_sub(1),
            Event::Start(Tag::Strikethrough) => self.strike += 1,
            Event::End(TagEnd::Strikethrough) => self.strike = self.strike.saturating_sub(1),

            Event::Start(Tag::Heading { level, .. }) => self.heading = Some(level_num(level)),
            Event::End(TagEnd::Heading(_)) => {
                self.flush_block();
                self.blank();
                self.heading = None;
            }

            Event::Start(Tag::BlockQuote(_)) => {
                self.quote_depth += 1;
                self.indent.push(format!("{}▎ ", self.cur_indent()));
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.indent.pop();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                if self.quote_depth == 0 {
                    self.blank();
                }
            }

            Event::Start(Tag::List(first)) => {
                self.flush_block();
                self.lists.push(ListState { next: first });
            }
            Event::End(TagEnd::List(_)) => {
                self.lists.pop();
                if self.lists.is_empty() {
                    self.blank();
                }
            }
            Event::Start(Tag::Item) => {
                let base = self.cur_indent();
                let marker = match self.lists.last_mut() {
                    Some(state) => match state.next {
                        Some(n) => {
                            state.next = Some(n + 1);
                            format!("{n}. ")
                        }
                        None => "• ".to_string(),
                    },
                    None => "• ".to_string(),
                };
                let pad = " ".repeat(marker.chars().count());
                self.pending_first = Some(format!("{base}{marker}"));
                self.indent.push(format!("{base}{pad}"));
            }
            Event::End(TagEnd::Item) => {
                self.flush_block();
                self.indent.pop();
            }

            Event::Start(Tag::Link { dest_url, .. }) => {
                self.urls.push(dest_url.to_string());
                self.link += 1;
            }
            Event::End(TagEnd::Link) => {
                self.link = self.link.saturating_sub(1);
                if let Some(url) = self.urls.pop() {
                    self.push_text(&format!(" ({url})"), Style::new().fg(Color::DarkGray));
                }
            }

            Event::Rule => self.rule(),

            Event::Start(Tag::CodeBlock(kind)) => {
                self.flush_block();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((lang, src)) = self.code.take() {
                    self.render_code(&lang, &src);
                    self.blank();
                }
            }

            Event::Start(Tag::Table(_)) => {
                self.flush_block();
                self.table = Some(TableBuilder::default());
            }
            Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => {
                if let Some(t) = &mut self.table {
                    t.current.clear();
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(t) = &mut self.table {
                    t.cell.clear();
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(t) = &mut self.table {
                    let cell = t.cell.trim().to_string();
                    t.current.push(cell);
                }
            }
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => {
                if let Some(t) = &mut self.table {
                    let row = std::mem::take(&mut t.current);
                    t.rows.push(row);
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(t) = self.table.take() {
                    self.render_table(t);
                    self.blank();
                }
            }

            Event::Text(text) => {
                if let Some((_, buf)) = &mut self.code {
                    buf.push_str(&text);
                } else if let Some(t) = &mut self.table {
                    t.cell.push_str(&text);
                } else {
                    self.push_text(&text, Style::new());
                }
            }
            Event::Code(code) => {
                if let Some(t) = &mut self.table {
                    t.cell.push_str(&code);
                } else {
                    self.push_text(&code, Style::new().fg(Color::Cyan));
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(t) = &mut self.table {
                    t.cell.push(' ');
                } else {
                    self.push_text(" ", Style::new());
                }
            }
            Event::End(TagEnd::Paragraph) => {
                self.flush_block();
                if self.lists.is_empty() && self.quote_depth == 0 {
                    self.blank();
                }
            }
            _ => {}
        }
    }

    fn flush_block(&mut self) {
        if self.inline.is_empty() {
            self.pending_first = None;
            return;
        }
        let tokens = std::mem::take(&mut self.inline);
        let cont = self.cur_indent();
        let first = self.pending_first.take().unwrap_or_else(|| cont.clone());
        self.out.extend(wrap_tokens(&tokens, self.width, &first, &cont));
    }

    fn blank(&mut self) {
        if matches!(self.out.last(), Some(l) if !l.spans.is_empty()) {
            self.out.push(Line::default());
        }
    }

    fn rule(&mut self) {
        let indent = self.cur_indent();
        let fill = self.width.saturating_sub(indent.chars().count()).max(1);
        let bar = "─".repeat(fill);
        self.out.push(Line::from(vec![
            Span::raw(indent),
            Span::styled(bar, Style::new().fg(Color::DarkGray)),
        ]));
        self.blank();
    }

    fn render_code(&mut self, lang: &str, src: &str) {
        let indent = self.cur_indent();
        let avail = self.width.saturating_sub(indent.chars().count()).max(1);
        let body = src.strip_suffix('\n').unwrap_or(src);
        for line in body.split('\n') {
            let tokens = highlight::highlight(lang, line);
            for row in hard_wrap_tokens(&tokens, avail) {
                let mut spans: Vec<Span<'static>> = Vec::new();
                if !indent.is_empty() {
                    spans.push(Span::raw(indent.clone()));
                }
                let mut used = 0;
                for (style, text) in row {
                    used += text.chars().count();
                    spans.push(Span::styled(text, style.bg(CODE_BG)));
                }
                if used < avail {
                    spans.push(Span::styled(
                        " ".repeat(avail - used),
                        Style::new().bg(CODE_BG),
                    ));
                }
                self.out.push(Line::from(spans));
            }
        }
    }

    fn render_table(&mut self, t: TableBuilder) {
        let rows = t.rows;
        let ncols = rows.iter().map(Vec::len).max().unwrap_or(0);
        if ncols == 0 {
            return;
        }
        let mut widths = vec![0usize; ncols];
        for row in &rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
        let indent = self.cur_indent();
        let avail = self.width.saturating_sub(indent.chars().count()).max(1);
        let sep_total = 3 * ncols.saturating_sub(1);
        shrink_widths(&mut widths, avail.saturating_sub(sep_total));

        self.push_table_row(&indent, &rows[0], &widths, true);
        self.push_table_rule(&indent, &widths);
        for row in &rows[1..] {
            self.push_table_row(&indent, row, &widths, false);
        }
    }

    fn push_table_row(&mut self, indent: &str, row: &[String], widths: &[usize], header: bool) {
        let sep = Style::new().fg(Color::DarkGray);
        let cell_style = if header {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new()
        };
        let mut spans: Vec<Span<'static>> = Vec::new();
        if !indent.is_empty() {
            spans.push(Span::raw(indent.to_string()));
        }
        for (i, &w) in widths.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" │ ", sep));
            }
            let text = row.get(i).map(String::as_str).unwrap_or("");
            spans.push(Span::styled(fit(text, w), cell_style));
        }
        self.out.push(Line::from(spans));
    }

    fn push_table_rule(&mut self, indent: &str, widths: &[usize]) {
        let style = Style::new().fg(Color::DarkGray);
        let mut spans: Vec<Span<'static>> = Vec::new();
        if !indent.is_empty() {
            spans.push(Span::raw(indent.to_string()));
        }
        for (i, &w) in widths.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("─┼─", style));
            }
            spans.push(Span::styled("─".repeat(w), style));
        }
        self.out.push(Line::from(spans));
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_block();
        if matches!(self.out.last(), Some(l) if l.spans.is_empty()) {
            self.out.pop();
        }
        self.out
    }
}

fn level_num(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn heading_color(level: u8) -> Color {
    match level {
        1 => Color::Magenta,
        2 => Color::Yellow,
        _ => Color::Green,
    }
}

fn wrap_tokens(tokens: &[Token], width: usize, first: &str, cont: &str) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut words: Vec<Token> = Vec::new();
    for (style, text) in tokens {
        for (i, part) in text.split(' ').enumerate() {
            if i > 0 {
                words.push((*style, " ".to_string()));
            }
            if !part.is_empty() {
                words.push((*style, part.to_string()));
            }
        }
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    let open = |lines: &[Line<'static>]| -> (Vec<Span<'static>>, usize) {
        let p = if lines.is_empty() { first } else { cont };
        if p.is_empty() {
            (Vec::new(), 0)
        } else {
            (vec![Span::raw(p.to_string())], p.chars().count())
        }
    };

    let (mut cur, mut cur_width) = open(&lines);
    let mut at_line_start = true;

    for (style, word) in words {
        let is_space = word == " ";
        let wlen = word.chars().count();

        if is_space {
            if at_line_start {
                continue;
            }
            cur.push(Span::styled(word, style));
            cur_width += 1;
            continue;
        }

        if !at_line_start && cur_width + wlen > width {
            if matches!(cur.last(), Some(s) if s.content.as_ref() == " ") {
                cur.pop();
            }
            lines.push(Line::from(std::mem::take(&mut cur)));
            let (s, w) = open(&lines);
            cur = s;
            cur_width = w;
            at_line_start = true;
        }

        if wlen > width {
            for chunk in chunk_chars(&word, width.saturating_sub(cur_width).max(1)) {
                let clen = chunk.chars().count();
                if !at_line_start && cur_width + clen > width {
                    lines.push(Line::from(std::mem::take(&mut cur)));
                    let (s, w) = open(&lines);
                    cur = s;
                    cur_width = w;
                }
                cur.push(Span::styled(chunk, style));
                cur_width += clen;
                at_line_start = false;
            }
            continue;
        }

        cur.push(Span::styled(word, style));
        cur_width += wlen;
        at_line_start = false;
    }

    if matches!(cur.last(), Some(s) if s.content.as_ref() == " ") {
        cur.pop();
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(Line::from(cur));
    }
    lines
}

fn hard_wrap_tokens(tokens: &[Token], width: usize) -> Vec<Vec<Token>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<Token>> = Vec::new();
    let mut cur: Vec<Token> = Vec::new();
    let mut cur_w = 0;
    for (style, text) in tokens {
        for ch in text.chars() {
            if cur_w == width {
                rows.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            match cur.last_mut() {
                Some(last) if last.0 == *style => last.1.push(ch),
                _ => cur.push((*style, ch.to_string())),
            }
            cur_w += 1;
        }
    }
    if !cur.is_empty() || rows.is_empty() {
        rows.push(cur);
    }
    rows
}

fn fit(s: &str, w: usize) -> String {
    let len = s.chars().count();
    if len == w {
        s.to_string()
    } else if len < w {
        format!("{s}{}", " ".repeat(w - len))
    } else if w == 0 {
        String::new()
    } else if w == 1 {
        "…".to_string()
    } else {
        let head: String = s.chars().take(w - 1).collect();
        format!("{head}…")
    }
}

fn shrink_widths(widths: &mut [usize], budget: usize) {
    let budget = budget.max(widths.len());
    while widths.iter().sum::<usize>() > budget {
        match widths.iter_mut().filter(|w| **w > 1).max_by_key(|w| **w) {
            Some(w) => *w -= 1,
            None => break,
        }
    }
}

fn chunk_chars(s: &str, n: usize) -> Vec<String> {
    let n = n.max(1);
    let chars: Vec<char> = s.chars().collect();
    chars.chunks(n).map(|c| c.iter().collect()).collect()
}

#[cfg(test)]
mod tests;

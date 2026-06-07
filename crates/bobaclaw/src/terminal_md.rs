//! Markdown → ANSI for terminal output (chat REPL, one-shot agent).

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

const RESET: &str = "\x1b[0m";

struct Style {
    open: &'static str,
}

const STYLE_H1: Style = Style { open: "\x1b[1;36m" };
const STYLE_H2: Style = Style { open: "\x1b[1;97m" };
const STYLE_H3: Style = Style { open: "\x1b[1;2m" };
const STYLE_H4: Style = Style { open: "\x1b[1m" };
const STRONG: Style = Style { open: "\x1b[1m" };
const EMPH: Style = Style { open: "\x1b[3m" };
const CODE: Style = Style {
    open: "\x1b[38;5;251m\x1b[48;5;236m",
};
const LINK: Style = Style { open: "\x1b[4;36m" };
const QUOTE: Style = Style {
    open: "\x1b[2;38;5;245m",
};
const CODE_BLOCK: Style = Style {
    open: "\x1b[38;5;252m\x1b[48;5;236m",
};
const RULE: Style = Style {
    open: "\x1b[2;38;5;238m",
};
const META: Style = Style {
    open: "\x1b[2;38;5;240m",
};

/// Render markdown into logical lines (may contain ANSI escapes when `color` is true).
pub fn render_markdown_lines(markdown: &str, color: bool) -> Vec<String> {
    let mut r = Renderer::new(color);
    let opts = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    for event in Parser::new_ext(markdown, opts) {
        r.on_event(event);
    }
    r.finish();
    if r.lines.is_empty() {
        vec!["(пустой ответ)".into()]
    } else {
        r.lines
    }
}

struct Renderer {
    color: bool,
    lines: Vec<String>,
    line: String,
    style_stack: Vec<&'static str>,
    in_code_block: bool,
    code_block_buf: String,
    list_stack: Vec<ListKind>,
    link_url: Option<String>,
    quote_depth: usize,
    ordered_index: usize,
}

#[derive(Clone, Copy)]
enum ListKind {
    Bullet,
    Ordered,
}

impl Renderer {
    fn new(color: bool) -> Self {
        Self {
            color,
            lines: Vec::new(),
            line: String::new(),
            style_stack: Vec::new(),
            in_code_block: false,
            code_block_buf: String::new(),
            list_stack: Vec::new(),
            link_url: None,
            quote_depth: 0,
            ordered_index: 0,
        }
    }

    fn finish(&mut self) {
        self.flush_line();
        if self.in_code_block {
            self.flush_code_block();
        }
    }

    fn on_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag_end) => self.end_tag(tag_end),
            Event::Text(text) => self.write(&text),
            Event::Code(text) => self.write_styled(&text, Some(CODE.open)),
            Event::SoftBreak => self.line.push(' '),
            Event::HardBreak => self.flush_line(),
            Event::Rule => {
                self.flush_line();
                self.push_line_styled("────────────────────────", &RULE);
            }
            Event::Html(_) | Event::InlineHtml(_) => {}
            _ => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush_line();
                self.style_stack.push(heading_style(level).open);
            }
            Tag::Paragraph => {
                if !self.in_code_block {
                    self.flush_line();
                }
            }
            Tag::Strong => self.style_stack.push(STRONG.open),
            Tag::Emphasis => self.style_stack.push(EMPH.open),
            Tag::Strikethrough => self.style_stack.push("\x1b[9m"),
            Tag::Link { dest_url, .. } => {
                self.link_url = Some(dest_url.to_string());
                self.style_stack.push(LINK.open);
            }
            Tag::CodeBlock(kind) => {
                self.flush_line();
                self.in_code_block = true;
                self.code_block_buf.clear();
                if let CodeBlockKind::Fenced(lang) = kind {
                    if !lang.is_empty() {
                        self.code_block_buf.push_str(&format!("// {lang}\n"));
                    }
                }
            }
            Tag::BlockQuote(_) => {
                self.flush_line();
                self.quote_depth += 1;
                self.style_stack.push(QUOTE.open);
            }
            Tag::List(start) => {
                self.flush_line();
                self.list_stack.push(if start.is_some() {
                    ListKind::Ordered
                } else {
                    ListKind::Bullet
                });
                self.ordered_index = start.map(|n| n as usize).unwrap_or(0).saturating_sub(1);
            }
            Tag::Item => {
                self.flush_line();
                let prefix = match self.list_stack.last() {
                    Some(ListKind::Bullet) => "  • ".to_string(),
                    Some(ListKind::Ordered) => {
                        self.ordered_index += 1;
                        format!("  {}. ", self.ordered_index)
                    }
                    None => "  • ".to_string(),
                };
                self.line.push_str(&prefix);
            }
            Tag::Table(_) => self.flush_line(),
            Tag::TableHead | Tag::TableRow | Tag::TableCell => {}
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.pop_style();
                self.flush_line();
                self.lines.push(String::new());
            }
            TagEnd::Paragraph => {
                if !self.in_code_block {
                    self.flush_line();
                }
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link => {
                if let Some(url) = self.link_url.take() {
                    if self.color {
                        self.line.push_str(&format!("{RESET}"));
                        self.write_styled(&format!(" ({url})"), Some(META.open));
                    } else {
                        self.line.push_str(&format!(" ({url})"));
                    }
                }
                self.pop_style();
            }
            TagEnd::CodeBlock => {
                self.flush_code_block();
                self.lines.push(String::new());
            }
            TagEnd::BlockQuote(_) => {
                self.pop_style();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.flush_line();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.lines.push(String::new());
            }
            TagEnd::Item => self.flush_line(),
            TagEnd::Table => self.flush_line(),
            TagEnd::TableHead | TagEnd::TableRow | TagEnd::TableCell => {}
            _ => {}
        }
    }

    fn flush_code_block(&mut self) {
        self.in_code_block = false;
        let block: Vec<String> = self.code_block_buf.lines().map(|s| s.to_string()).collect();
        self.code_block_buf.clear();
        for line in block {
            self.push_line_styled(&line, &CODE_BLOCK);
        }
    }

    fn write(&mut self, text: &str) {
        if self.in_code_block {
            self.code_block_buf.push_str(text);
            return;
        }
        let quote = if self.quote_depth > 0 { "│ " } else { "" };
        if !quote.is_empty() && self.line.is_empty() {
            self.line.push_str(quote);
        }
        self.write_styled(text, self.style_stack.last().copied());
    }

    fn write_styled(&mut self, text: &str, style_open: Option<&'static str>) {
        if let Some(open) = style_open {
            if self.color {
                self.line.push_str(open);
            }
        }
        self.line.push_str(text);
        if style_open.is_some() && self.color {
            self.line.push_str(RESET);
        }
    }

    fn push_line_styled(&mut self, text: &str, style: &Style) {
        let mut line = String::new();
        if self.color {
            line.push_str(style.open);
        }
        line.push_str(text);
        if self.color {
            line.push_str(RESET);
        }
        self.lines.push(line);
    }

    fn flush_line(&mut self) {
        let trimmed = self.line.trim_end();
        if !trimmed.is_empty() {
            self.lines.push(trimmed.to_string());
        }
        self.line.clear();
    }

    fn pop_style(&mut self) {
        self.style_stack.pop();
        if self.color {
            self.line.push_str(RESET);
        }
    }
}

fn heading_style(level: pulldown_cmark::HeadingLevel) -> Style {
    use pulldown_cmark::HeadingLevel::*;
    match level {
        H1 => STYLE_H1,
        H2 => STYLE_H2,
        H3 => STYLE_H3,
        _ => STYLE_H4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_heading_and_bold() {
        let lines = render_markdown_lines("# Title\n\n**bold** text", true);
        assert!(lines.iter().any(|l| l.contains("Title")));
        assert!(lines.iter().any(|l| l.contains("bold")));
    }

    #[test]
    fn code_block_preserved() {
        let md = "text\n\n```rust\nfn main() {}\n```";
        let lines = render_markdown_lines(md, false);
        let joined = lines.join("\n");
        assert!(joined.contains("fn main"));
    }

    #[test]
    fn empty_yields_placeholder() {
        let lines = render_markdown_lines("   ", false);
        assert_eq!(lines, vec!["(пустой ответ)"]);
    }

    #[test]
    fn no_color_skips_ansi() {
        let lines = render_markdown_lines("**x**", false);
        assert!(!lines.iter().any(|l| l.contains("\x1b[")));
    }
}

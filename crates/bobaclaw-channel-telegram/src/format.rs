//! Markdown → Telegram HTML (GramIO-style: standard MD, not MarkdownV2).
//! See https://gramio.dev/formatting — LLM markdown maps to Bot API HTML tags.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegramFormatMode {
    Plain,
    Html,
}

#[derive(Debug, Clone)]
pub struct FormattedMessage {
    pub text: String,
    pub parse_mode: Option<&'static str>,
}

pub fn format_for_telegram(markdown: &str, mode: TelegramFormatMode) -> FormattedMessage {
    match mode {
        TelegramFormatMode::Plain => FormattedMessage {
            text: markdown.to_string(),
            parse_mode: None,
        },
        TelegramFormatMode::Html => {
            let html = markdown_to_telegram_html(markdown);
            FormattedMessage {
                text: html,
                parse_mode: Some("HTML"),
            }
        }
    }
}

/// Convert common LLM markdown to [Telegram HTML](https://core.telegram.org/bots/api#html-style).
pub fn markdown_to_telegram_html(markdown: &str) -> String {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut html = HtmlRenderer::new();
    for event in Parser::new_ext(markdown, opts) {
        html.on_event(event);
    }
    html.finish()
}

struct HtmlRenderer {
    out: String,
    tag_stack: Vec<&'static str>,
    in_pre: bool,
    pre_lang: Option<String>,
    pre_buf: String,
    list_depth: usize,
    ordered_counters: Vec<usize>,
    link_url: Option<String>,
    blockquote_depth: usize,
}

impl HtmlRenderer {
    fn new() -> Self {
        Self {
            out: String::new(),
            tag_stack: Vec::new(),
            in_pre: false,
            pre_lang: None,
            pre_buf: String::new(),
            list_depth: 0,
            ordered_counters: Vec::new(),
            link_url: None,
            blockquote_depth: 0,
        }
    }

    fn finish(mut self) -> String {
        self.flush_pre();
        while self.tag_stack.pop().is_some() {
            // safety close
        }
        self.out.trim().to_string()
    }

    fn push_open(&mut self, tag: &'static str) {
        self.out.push_str(tag);
        self.tag_stack.push(tag);
    }

    fn pop_matching(&mut self, close: &str) {
        if let Some(pos) = self.tag_stack.iter().rposition(|t| *t == close) {
            for _ in pos..self.tag_stack.len() {
                let open = self.tag_stack.pop().unwrap();
                self.out.push_str(closing_tag(open));
            }
        }
    }

    fn on_event(&mut self, event: Event<'_>) {
        if self.in_pre {
            return self.on_pre_event(event);
        }
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    self.newline_if_needed();
                    let open = match level {
                        pulldown_cmark::HeadingLevel::H1
                        | pulldown_cmark::HeadingLevel::H2
                        | pulldown_cmark::HeadingLevel::H3
                        | pulldown_cmark::HeadingLevel::H4
                        | pulldown_cmark::HeadingLevel::H5
                        | pulldown_cmark::HeadingLevel::H6 => "<b>",
                    };
                    self.push_open(open);
                }
                Tag::Strong => self.push_open("<b>"),
                Tag::Emphasis => self.push_open("<i>"),
                Tag::Strikethrough => self.push_open("<s>"),
                Tag::Link { dest_url, .. } => {
                    self.link_url = Some(dest_url.to_string());
                    self.out.push_str(r#"<a href=""#);
                    escape_html_attr_into(&mut self.out, dest_url.as_ref());
                    self.out.push_str(r#"">"#);
                    self.tag_stack.push("<a>");
                }
                Tag::BlockQuote(_) => {
                    self.newline_if_needed();
                    self.push_open("<blockquote>");
                    self.blockquote_depth += 1;
                }
                Tag::List(start) => {
                    self.newline_if_needed();
                    self.list_depth += 1;
                    if start.is_some() {
                        self.ordered_counters.push(0);
                    } else {
                        self.ordered_counters.push(usize::MAX);
                    }
                }
                Tag::Item => {
                    self.newline_if_needed();
                    let prefix = if self.ordered_counters.last().copied() == Some(usize::MAX) {
                        "• ".to_string()
                    } else {
                        let n = self.ordered_counters.last_mut().unwrap();
                        *n += 1;
                        format!("{n}. ")
                    };
                    self.out.push_str(&escape_html_text(&prefix));
                }
                Tag::CodeBlock(kind) => {
                    self.flush_line_break();
                    self.in_pre = true;
                    self.pre_buf.clear();
                    self.pre_lang = match kind {
                        CodeBlockKind::Fenced(lang) if !lang.is_empty() => Some(lang.to_string()),
                        _ => None,
                    };
                }
                Tag::Table(_) | Tag::TableRow | Tag::TableCell => {}
                Tag::Paragraph => {}
                Tag::HtmlBlock => {}
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => self.pop_matching("<b>"),
                TagEnd::Strong => self.pop_matching("<b>"),
                TagEnd::Emphasis => self.pop_matching("<i>"),
                TagEnd::Strikethrough => self.pop_matching("<s>"),
                TagEnd::Link => {
                    self.link_url = None;
                    self.pop_matching("<a>");
                }
                TagEnd::BlockQuote(_) => {
                    self.pop_matching("<blockquote>");
                    self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                }
                TagEnd::List(_) => {
                    self.list_depth = self.list_depth.saturating_sub(1);
                    self.ordered_counters.pop();
                    self.out.push('\n');
                }
                TagEnd::Item => {}
                TagEnd::CodeBlock => self.flush_pre(),
                TagEnd::Table | TagEnd::TableRow | TagEnd::TableCell | TagEnd::Paragraph => {
                    self.out.push('\n');
                }
                _ => {}
            },
            Event::Text(t) => self.push_text(t.as_ref()),
            Event::Code(t) => {
                self.push_open("<code>");
                self.push_text(t.as_ref());
                self.pop_matching("<code>");
            }
            Event::SoftBreak | Event::HardBreak => self.out.push('\n'),
            Event::Rule => {
                self.newline_if_needed();
                self.out.push_str("────────\n");
            }
            Event::InlineHtml(h) | Event::Html(h) => self.out.push_str(h.as_ref()),
            _ => {}
        }
    }

    fn on_pre_event(&mut self, event: Event<'_>) {
        match event {
            Event::End(TagEnd::CodeBlock) => self.flush_pre(),
            Event::Code(t) | Event::Text(t) => self.pre_buf.push_str(t.as_ref()),
            Event::SoftBreak | Event::HardBreak => self.pre_buf.push('\n'),
            _ => {}
        }
    }

    fn flush_pre(&mut self) {
        if !self.in_pre {
            return;
        }
        self.in_pre = false;
        if let Some(lang) = self.pre_lang.take() {
            self.out.push_str("<pre><code class=\"language-");
            escape_html_attr_into(&mut self.out, &lang);
            self.out.push_str("\">");
        } else {
            self.out.push_str("<pre><code>");
        }
        self.out.push_str(&escape_html_text(&self.pre_buf));
        self.out.push_str("</code></pre>\n");
        self.pre_buf.clear();
    }

    fn push_text(&mut self, t: &str) {
        self.out.push_str(&escape_html_text(t));
    }

    fn newline_if_needed(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn flush_line_break(&mut self) {
        self.newline_if_needed();
    }
}

fn closing_tag(open: &str) -> &'static str {
    match open {
        "<b>" => "</b>",
        "<i>" => "</i>",
        "<s>" => "</s>",
        "<code>" => "</code>",
        "<a>" => "</a>",
        "<blockquote>" => "</blockquote>",
        _ => "",
    }
}

fn escape_html_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_html_attr_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_and_link() {
        let html = markdown_to_telegram_html("**Hi** [GramIO](https://gramio.dev)");
        assert!(html.contains("<b>Hi</b>"));
        assert!(html.contains(r#"<a href="https://gramio.dev">GramIO</a>"#));
    }

    #[test]
    fn code_block() {
        let html = markdown_to_telegram_html("```rust\nlet x = 1;\n```");
        assert!(html.contains("<pre><code"));
        assert!(html.contains("let x = 1;"));
    }

    #[test]
    fn escapes_raw_html() {
        let html = markdown_to_telegram_html("a < b & c");
        assert!(html.contains("&lt;"));
        assert!(html.contains("&amp;"));
    }
}

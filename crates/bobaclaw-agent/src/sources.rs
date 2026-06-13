//! Collect URLs and workspace paths from tool calls and append a Sources footer when missing.

use std::collections::HashSet;

use bobaclaw_provider::ToolCall;
use reqwest::Url;
use serde_json::Value;

use crate::tools::{MEMORY_READ, MEMORY_SEARCH, WEB_FETCH};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TurnSource {
    Url(String),
    WorkspacePath(String),
}

/// Gather citable sources from a successful tool invocation.
pub fn collect_turn_sources(call: &ToolCall, result_body: &str, exit_code: i32) -> Vec<TurnSource> {
    if exit_code != 0 {
        return Vec::new();
    }
    let mut out = collect_from_tool_args(call);
    out.extend(collect_from_tool_result(
        call.function.name.as_str(),
        result_body,
    ));
    dedupe_sources(out)
}

/// Append `## Sources` when the model omitted it but tools produced citable references.
pub fn append_sources_if_missing(text: &str, sources: &[TurnSource]) -> String {
    if sources.is_empty() || text.to_lowercase().contains("## sources") {
        return text.to_string();
    }

    let mut lines = Vec::new();
    for source in sources {
        if source_already_in_text(text, source) {
            continue;
        }
        lines.push(format_source_line(source));
    }

    if lines.is_empty() {
        return text.to_string();
    }

    let mut out = text.trim_end().to_string();
    out.push_str("\n\n## Sources\n\n");
    out.push_str(&lines.join("\n"));
    out
}

fn collect_from_tool_args(call: &ToolCall) -> Vec<TurnSource> {
    let args = parse_args(&call.function.arguments);
    let name = call.function.name.as_str();
    let mut out = Vec::new();

    if name == WEB_FETCH || name.ends_with("browser_navigate") || name.contains("browser_navigate")
    {
        if let Some(url) = arg_string(&args, "url") {
            push_url(&mut out, &url);
        }
    } else if name == MEMORY_READ {
        if let Some(path) = arg_string(&args, "path") {
            push_workspace_path(&mut out, &path);
        }
    } else if name.starts_with("mcp_") {
        if let Some(url) = arg_string(&args, "url") {
            push_url(&mut out, &url);
        }
    }

    out
}

fn collect_from_tool_result(tool_name: &str, body: &str) -> Vec<TurnSource> {
    let mut out = Vec::new();

    if tool_name == WEB_FETCH {
        if let Some(url) = parse_prefixed_line(body, "url:") {
            push_url(&mut out, &url);
        }
    }

    if tool_name == MEMORY_SEARCH {
        out.extend(parse_memory_search_paths(body));
    }

    dedupe_sources(out)
}

fn parse_memory_search_paths(body: &str) -> Vec<TurnSource> {
    let mut in_files = false;
    let mut out = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed == "## Memory files" {
            in_files = true;
            continue;
        }
        if trimmed.starts_with("## ") {
            in_files = false;
            continue;
        }
        if !in_files {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("- ") else {
            continue;
        };
        let Some((path, _snippet)) = rest.split_once(':') else {
            continue;
        };
        push_workspace_path(&mut out, path.trim());
    }
    out
}

fn parse_prefixed_line(body: &str, prefix: &str) -> Option<String> {
    body.lines()
        .find(|l| l.trim().starts_with(prefix))
        .map(|l| l.trim()[prefix.len()..].trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_args(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or(Value::Null)
}

fn arg_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn push_url(out: &mut Vec<TurnSource>, url: &str) {
    if looks_like_http_url(url) {
        out.push(TurnSource::Url(url.to_string()));
    }
}

fn push_workspace_path(out: &mut Vec<TurnSource>, path: &str) {
    let normalized = path.replace('\\', "/");
    if normalized == "MEMORY.md" || normalized.starts_with("memory/") {
        out.push(TurnSource::WorkspacePath(normalized));
    }
}

fn looks_like_http_url(s: &str) -> bool {
    Url::parse(s)
        .ok()
        .map(|u| u.scheme() == "http" || u.scheme() == "https")
        .unwrap_or(false)
}

fn format_source_line(source: &TurnSource) -> String {
    match source {
        TurnSource::Url(url) => {
            let label = Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| url.clone());
            format!("- [{label}]({url})")
        }
        TurnSource::WorkspacePath(path) => format!("- [{path}]({path})"),
    }
}

fn source_already_in_text(text: &str, source: &TurnSource) -> bool {
    match source {
        TurnSource::Url(url) => text.contains(url.as_str()),
        TurnSource::WorkspacePath(path) => text.contains(path.as_str()),
    }
}

fn dedupe_sources(sources: Vec<TurnSource>) -> Vec<TurnSource> {
    let mut seen = HashSet::new();
    sources
        .into_iter()
        .filter(|s| seen.insert(s.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bobaclaw_provider::FunctionCallPayload;

    fn tool_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "1".into(),
            kind: "function".into(),
            function: FunctionCallPayload {
                name: name.into(),
                arguments: args.to_string(),
            },
        }
    }

    #[test]
    fn collects_web_fetch_url() {
        let call = tool_call(
            WEB_FETCH,
            serde_json::json!({"url": "https://example.com/doc"}),
        );
        let sources = collect_turn_sources(&call, "url: https://example.com/doc\n\nbody", 0);
        assert!(sources
            .iter()
            .any(|s| matches!(s, TurnSource::Url(u) if u.contains("example.com"))));
    }

    #[test]
    fn collects_browser_navigate_url() {
        let call = tool_call(
            "mcp_obscura_browser_navigate",
            serde_json::json!({"url": "https://news.ycombinator.com"}),
        );
        let sources = collect_turn_sources(&call, "ok", 0);
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn collects_memory_read_path() {
        let call = tool_call(MEMORY_READ, serde_json::json!({"path": "MEMORY.md"}));
        let sources = collect_turn_sources(&call, "fact", 0);
        assert!(matches!(&sources[0], TurnSource::WorkspacePath(p) if p == "MEMORY.md"));
    }

    #[test]
    fn append_sources_when_missing() {
        let sources = vec![TurnSource::Url("https://example.com".into())];
        let out = append_sources_if_missing("Answer text.", &sources);
        assert!(out.contains("## Sources"));
        assert!(out.contains("[example.com](https://example.com)"));
    }

    #[test]
    fn skip_append_when_model_cited() {
        let sources = vec![TurnSource::Url("https://example.com".into())];
        let text = "Done.\n\n## Sources\n\n- [example.com](https://example.com)";
        assert_eq!(append_sources_if_missing(text, &sources), text);
    }

    #[test]
    fn skips_failed_tool_calls() {
        let call = tool_call(WEB_FETCH, serde_json::json!({"url": "https://example.com"}));
        assert!(collect_turn_sources(&call, "error", 1).is_empty());
    }
}

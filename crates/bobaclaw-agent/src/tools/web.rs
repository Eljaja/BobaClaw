use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use bobaclaw_core::WebFetchConfig;
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use reqwest::redirect::Policy;
use reqwest::Url;
use serde::Deserialize;
use serde_json::json;

pub const WEB_FETCH: &str = "web_fetch";

const MAX_URL_LEN: usize = 2048;

pub fn web_fetch_tool_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: WEB_FETCH.into(),
            description: "Fetch a public HTTP(S) URL as text (HTML reduced to plain text). \
                Host-side — not sandboxed. When you use fetched content in the answer, cite the URL in Sources."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "http or https URL" }
                },
                "required": ["url"]
            }),
        },
    }
}

pub fn is_web_fetch_tool(name: &str) -> bool {
    name == WEB_FETCH
}

#[derive(Debug, Deserialize)]
struct WebFetchArgs {
    url: String,
}

pub async fn handle_web_fetch_tool(
    cfg: &WebFetchConfig,
    call: &ToolCall,
) -> anyhow::Result<String> {
    if !cfg.enabled {
        anyhow::bail!("web_fetch is disabled — set tools.web_fetch.enabled: true in config.yaml");
    }

    let args: WebFetchArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid web_fetch arguments: {e}"))?;

    let url_str = args.url.trim();
    if url_str.is_empty() {
        anyhow::bail!("url must not be empty");
    }
    if url_str.len() > MAX_URL_LEN {
        anyhow::bail!("url exceeds max length ({MAX_URL_LEN})");
    }

    let url = Url::parse(url_str).map_err(|e| anyhow::anyhow!("invalid url: {e}"))?;
    validate_fetch_url(&url)?;

    let client = reqwest::Client::builder()
        .redirect(Policy::limited(cfg.max_redirects))
        .timeout(Duration::from_secs(cfg.timeout_secs))
        .build()?;

    let resp = client.get(url.clone()).send().await?;
    validate_fetch_url(resp.url())?;

    let status = resp.status();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    if !content_type.is_empty()
        && !content_type.starts_with("text/")
        && !content_type.contains("json")
        && !content_type.contains("xml")
    {
        anyhow::bail!("unsupported content-type: {content_type}");
    }

    let bytes = resp.bytes().await?;
    if bytes.len() > cfg.max_bytes {
        anyhow::bail!(
            "response exceeds max size ({} bytes, limit {})",
            bytes.len(),
            cfg.max_bytes
        );
    }

    let body = String::from_utf8_lossy(&bytes);
    let text = if content_type.contains("html") || looks_like_html(&body) {
        html_to_text(&body)
    } else {
        body.into_owned()
    };

    Ok(format!(
        "url: {url_str}\nstatus: {status}\ncontent-type: {content_type}\n\n{text}"
    ))
}

fn looks_like_html(s: &str) -> bool {
    let lower = s.get(..512).unwrap_or(s).to_ascii_lowercase();
    lower.contains("<html") || lower.contains("<!doctype html")
}

fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    collapse_blank_lines(&out)
}

fn collapse_blank_lines(s: &str) -> String {
    let mut lines = Vec::new();
    let mut prev_blank = false;
    for line in s.lines() {
        let trimmed = line.trim();
        let blank = trimmed.is_empty();
        if blank && prev_blank {
            continue;
        }
        lines.push(trimmed);
        prev_blank = blank;
    }
    lines.join("\n")
}

fn validate_fetch_url(url: &Url) -> anyhow::Result<()> {
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        anyhow::bail!("only http and https URLs are allowed");
    }

    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("url missing host"))?;
    let host_lower = host.to_ascii_lowercase();
    if host_lower == "localhost"
        || host_lower.ends_with(".localhost")
        || host_lower.ends_with(".local")
    {
        anyhow::bail!("blocked host: {host}");
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_ip(ip) {
            anyhow::bail!("blocked IP: {ip}");
        }
    }

    Ok(())
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_ipv4(v4),
        IpAddr::V6(v6) => is_blocked_ipv6(v6),
    }
}

fn is_blocked_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.octets()[0] == 0
}

fn is_blocked_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || ip.is_unspecified() || ip.is_unique_local()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_localhost() {
        let url = Url::parse("http://localhost/test").unwrap();
        assert!(validate_fetch_url(&url).is_err());
    }

    #[test]
    fn blocks_private_ip_literal() {
        let url = Url::parse("http://192.168.1.1/").unwrap();
        assert!(validate_fetch_url(&url).is_err());
    }

    #[test]
    fn allows_public_https() {
        let url = Url::parse("https://example.com/").unwrap();
        assert!(validate_fetch_url(&url).is_ok());
    }

    #[test]
    fn html_strips_tags() {
        let text = html_to_text("<p>Hello <b>world</b></p>");
        assert!(text.contains("Hello"));
        assert!(text.contains("world"));
        assert!(!text.contains('<'));
    }
}

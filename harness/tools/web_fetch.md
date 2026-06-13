# Tool: web_fetch

Host-side HTTP GET for public URLs. Implementation: `crates/bobaclaw-agent/src/tools/web.rs`.

## Purpose

Fetch a URL as plain text (HTML stripped). Lighter than browser MCP for static pages. **Off by default** — enable in config.

## Config

```yaml
tools:
  web_fetch:
    enabled: false
    max_bytes: 524288
    timeout_secs: 30
    max_redirects: 5
```

## Input

```json
{ "url": "https://example.com/page" }
```

## Side effects

- Outbound HTTP from the gateway host (not sandboxed)
- SSRF mitigations: private/loopback IPs and `.local` hosts blocked

## Approval requirements

- Disabled unless `tools.web_fetch.enabled: true`

## Answer contract

When using fetched content in the user-facing answer, cite the URL in `## Sources`.

## Tests

`cargo test -p bobaclaw-agent web`

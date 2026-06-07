# Tool: MCP (dynamic)

## Purpose

Proxy to configured MCP servers (e.g. Obscura browser). Tool names and schemas are discovered at runtime from `McpHub`.

## Non-goals

- Do not assume MCP tools exist — check `bobaclaw doctor` / config.
- Do not use browser automation for repo harness work unless explicitly requested.

## Input / output

Per-server tool schema from MCP handshake. Output: text body or `MCP error: ...`.

## Side effects

Depends on registered server:

- **Obscura**: network, browser state, possible screenshots.
- Other servers: declare in server-specific docs when added.

## Approval requirements

- High risk by default (network + external systems).
- Operator must enable MCP in `~/.bobaclaw/config.yaml`.
- Browser/credential tools require explicit user intent.

## Timeouts and retries

MCP client timeout per hub config. Agent retries after reading error; no blind repeat.

## Failure modes

| Error | Agent response |
|-------|----------------|
| Unknown tool name | List available MCP tools or fix name |
| Docker MCP container failure | Report error; suggest `bobaclaw doctor` |
| Navigation blocked | Try alternate approach; stop after repeated failure |

## Telemetry

Progress: `ToolStart` / `ToolEnd` with sanitized preview. Full MCP payload may be large — truncate in UI.

## Tests

`scripts/test-obscura-navigate.sh`, MCP probes in `scripts/run-mcp-probes.sh`.

Implementation: `crates/bobaclaw-agent/src/tools/mcp.rs`, `crates/bobaclaw-mcp/`.

When adding a new MCP server, add a subsection here or `harness/tools/mcp-<server>.md`.

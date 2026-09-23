# Channel: Web UI

## Purpose

Local browser chat UI for the operator. `GET /ui` serves one self-contained HTML page; the page talks to `/api/web/*` (JSON + Server-Sent Events) to list, create and open conversations and to run agent turns with live progress.

Two ways to run it (both require `channels.web.enabled: true`):

| Mode | Command | Address |
|------|---------|---------|
| Gateway | `bobaclaw gateway start` | `http://<gateway.bind>:<gateway.port>/ui` (default `127.0.0.1:18790`) |
| Standalone | `bobaclaw channel web start` | `http://<channels.web.bind>:<channels.web.port>/ui` (default `127.0.0.1:18791`), plus `GET /health` |

Implementation: `crates/bobaclaw-channel-web/`. The gateway mounts it with `bobaclaw_channel_web::mount` inside the same axum `Router`, so any layer applied to the gateway app afterwards also covers the web routes.

## Non-goals

- Attachments / file upload (text only).
- Multi-user accounts, CORS, or cross-origin embedding.
- Replaying tool-call blocks from history (live turns only; history shows final text).
- Push delivery of spawn completions or scheduled messages into the open page.

## Config (`channels.web`)

| Key | Default | Meaning |
|-----|---------|---------|
| `enabled` | `false` | Mount into gateway / allow `channel web start` |
| `bind` | `127.0.0.1` | Standalone bind address |
| `port` | `18791` | Standalone port |
| `auth_token_env` | `BOBACLAW_GATEWAY_TOKEN` | Env var holding the bearer token |
| `title` | `BobaClaw` | Page title / header (HTML-escaped) |

## Auth contract

- `GET /ui` (and `/ui/`) needs no auth: the page contains no data.
- Every `/api/web/*` route requires `Authorization: Bearer <token>` when the env var named by `auth_token_env` is set and non-empty. Comparison is constant-time. Failure → `401 {"error":"unauthorized"}` with `WWW-Authenticate: Bearer`.
- No token configured: requests are allowed only because startup guarantees a loopback bind; a `WARN` is logged.
- Startup refuses a non-loopback bind without a token: standalone checks `channels.web.bind`, the gateway checks `gateway.bind` (the gateway fails to start rather than exposing the UI).
- The page asks for the token once after a `401`, stores it in `localStorage` (`bobaclaw.web.token`), and only ever sends it in the `Authorization` header — never in a URL.

## Browser security

`/ui` response headers:

- `Content-Security-Policy: default-src 'none'; script-src 'nonce-<n>'; style-src 'nonce-<n>'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'` (fresh nonce per request)
- `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, `Cache-Control: no-store`
- No CORS headers (same-origin only).

Rendering: user text is set via `textContent`. Assistant markdown is rendered by a small in-page renderer that HTML-escapes the whole input first, then adds code blocks, inline code, bold/italic/strike, headings, lists, blockquotes, tables and links (`http`/`https` targets only, `rel="noopener noreferrer nofollow"`). Tool names, labels and previews are set via `textContent`.

## API

All paths are relative to the server root. Sessions are scoped to `source = "web"` and `default_agent_group`; any other session id returns `404`.

| Method | Path | Body | Response |
|--------|------|------|----------|
| GET | `/api/web/config` | — | `{title, agent_group}` |
| GET | `/api/web/sessions` | — | `[{id, title, started_at, updated_at, message_count}]`, most recent first (max 200) |
| POST | `/api/web/sessions` | — | `201` + session object |
| GET | `/api/web/sessions/{id}/messages` | — | `[{id, role, text, timestamp}]`; `role` ∈ `user`, `assistant`, `summary` |
| POST | `/api/web/sessions/{id}/messages` | `{"text": "..."}` (≤ 32 KiB, non-empty) | `text/event-stream` (see below) |
| POST | `/api/web/sessions/{id}/interrupt` | — | `{interrupted: bool}` → `AgentDispatcher::interrupt_session` |

Session `title` = stored title, else the first user message (whitespace collapsed, 60 chars), else `New chat`.

History display rules: assistant rows go through `sanitize_user_reply` (drops the `<!-- tool-results -->` appendix and leaked tool XML); `compaction` rows become `summary` markers (collapsed in the UI); other roles are dropped.

## Turn / SSE contract

A `POST .../messages` builds a `NormalizedRequest { ingress: Web, session_id: Some(id) }` and runs `AgentDispatcher::handle_with_progress` in a spawned task. `IngressKind::Web` is interactive: it **preempts** the in-flight turn on the same session (newest message wins), like CLI and Telegram.

Each SSE frame has `event: <type>` and `data: <json>` where the JSON carries the same `type`:

| `type` | Fields | Source |
|--------|--------|--------|
| `start` | `session_id` | Turn accepted |
| `thinking` | `iteration` | `AgentEvent::LlmThinking` |
| `tool_start` | `name`, `label` (≤ 500 chars) | `ToolStart` |
| `tool_end` | `name`, `exit_code`, `preview` (≤ 4000 chars) | `ToolEnd` |
| `compacting` | `tokens` | `Compacting` |
| `assistant_chunk` | `text` (sanitized) | `AssistantChunk` — intermediate, superseded by `done` |
| `retry` | `attempt`, `max_attempts` | `EmptyResponseRetry` |
| `interrupted` | — | `Interrupted` |
| `subagent_start` | `id`, `label` | `SubagentStart` |
| `subagent_end` | `id`, `exit_code`, `preview` | `SubagentEnd` |
| `done` | `reply` (sanitized), `session_id`, `run_id`, `interrupted` | Final frame on success |
| `error` | `message` | Final frame on failure (e.g. provider unreachable) |

Exactly one of `done` / `error` ends the stream. Keep-alive comments every 15 s. Events are pushed through an unbounded channel from the progress callback, so the agent loop never blocks on the browser.

## Side effects

- Creates `sessions` rows (`source = web`) and appends `messages` like any other ingress.
- Runs full agent turns (tools, sandbox exec, MCP, memory, skills, schedules, spawns).
- Spawn completions for web sessions: notice to the outbox (`deliver_channel = web`), wake replies land in the session history.

## Failure modes

| Situation | Behavior |
|-----------|----------|
| Browser disconnects mid-turn | Receiver dropped; the turn keeps running and is persisted; reopen the conversation to see it |
| Provider / turn error | `error` event, `WARN` log; the user message stays in history |
| Unknown / non-web session id | `404 {"error":"session not found"}` |
| Empty message | `400`; > 32 KiB → `413` |
| Non-loopback bind without token | Startup error (gateway and standalone) |

## Telemetry

- `INFO` on mount / listen address.
- `WARN` when no token is configured, and on failed turns (`web turn failed for session …`).

## Doctor

`bobaclaw doctor` prints `web ui: enabled=… standalone=… gateway=…` and, when enabled, `web token: OK`, a loopback-only warning, or `MISSING` for a non-loopback bind without a token.

## Tests

```bash
cargo test -p bobaclaw-channel-web   # auth, SSE serialization, router (oneshot) tests
cargo test -p bobaclaw-state session # list/create/history store methods
```

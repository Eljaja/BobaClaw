# BobaClaw: as-built architecture and feature list

**Status:** factual snapshot of runtime code (not roadmap)  
**Updated:** 2026-09-23  
**Audience:** operators and contributors  
**Related:** [ARCHITECTURE.md](ARCHITECTURE.md) (target design), [features.md](features.md) (comparison vs references — partially stale on subagents)

This document describes what is **actually implemented** in `crates/`. Where it diverges from [features.md](features.md), trust this file.

---

## Architecture (as implemented)

```mermaid
flowchart TB
  subgraph ingress [Ingress]
    CLI["CLI: agent / chat"]
    TG[Telegram long-poll]
    WEB["Web UI\n/ui + /api/web/* (SSE)"]
    GW["Gateway HTTP\n/health, /v1/chat/completions\n/api/agent, /api/spawn/*"]
  end

  subgraph control [Control plane]
    DISP[AgentDispatcher\nparallel sessions, serial per scope]
    SCHED[Scheduler\ncron + one-shot tasks]
    PAIR[Pairing / routing / policy]
  end

  subgraph runtime [Agent runtime — bobaclaw-agent]
    LOOP[AgentLoop]
    TURN[run_agent_turn\nLLM ↔ tools loop]
    COMP[Compaction\nLLM summarize]
    REV[Post-turn review\nmemory + skill background]
    SUB[SubagentManager\nnative / claude / codex / cursor]
  end

  subgraph tools [Tools]
    EXEC[exec]
    SCH_T[schedule_*]
    SKL[skill_* / memory_manage]
    SPAWN[subagent / spawn / spawn_status]
    MCP[mcp_* from McpHub]
  end

  subgraph exec_layer [Executor — bobaclaw-executor]
    BWRAP[bubblewrap]
    DOCKER[Docker container]
    LEDGER[Run ledger + capsules\n~/.bobaclaw/runs/]
  end

  subgraph state [Persistence — state.db SQLite WAL]
    SESS[sessions + messages]
    FTS[messages_fts FTS5]
    ROUTES[routes / pairing]
    CRON[cron_jobs + scheduled_tasks]
    SPAWN_J[spawn_jobs]
    RUNS[runs + run_events]
  end

  subgraph external [External]
    LLM[OpenAI-compatible API]
    MCPS[MCP servers\nstdio / HTTP]
  end

  CLI --> DISP
  TG --> PAIR --> DISP
  GW --> DISP
  WEB --> DISP
  SCHED --> DISP

  DISP --> LOOP --> TURN
  TURN --> COMP
  TURN --> tools
  TURN --> REV
  tools --> SUB
  EXEC --> BWRAP
  EXEC --> DOCKER
  EXEC --> LEDGER
  MCP --> MCPS
  TURN --> LLM

  LOOP --> SESS
  SUB --> SPAWN_J
  LEDGER --> RUNS
```

### Message flow (happy path)

1. **Ingress** → `NormalizedRequest` (CLI / Telegram / Web UI / REST / OpenAI-compat).
2. **Policy** (Telegram): pairing / allowlist / group rules → drop or pairing code.
3. **Routing**: `(channel, peer) → agent_group` from `config.yaml`.
4. **Session**: `SessionStore.resolve_session()` — history in SQLite.
5. **Dispatcher**: up to `max_parallel_turns` sessions in parallel; serialized within one scope; a new message **preempts** the in-flight turn (steering-like).
6. **Turn**: system prompt + history → LLM tool loop → sandbox `exec` / MCP / schedule / subagent.
7. **Persist**: assistant message + `<!-- tool-results -->` appendix.
8. **Background**: post-turn memory/skill review (async, Hermes-style).
9. **Outbound**: Telegram edit/stream or CLI outbox for scheduled delivery.

### Rust workspace (14 crates)

| Crate | Role |
|-------|------|
| `bobaclaw` | CLI binary (`clap`) |
| `bobaclaw-core` | Config, paths, routing, policy, request types, subagent config |
| `bobaclaw-state` | SQLite: sessions, runs, pairing, cron, spawn_jobs |
| `bobaclaw-provider` | OpenAI-compatible chat + tool calling |
| `bobaclaw-executor` | Sandbox: bubblewrap / Docker, run ledger |
| `bobaclaw-agent` | Agent loop, tools, compaction, subagents, review |
| `bobaclaw-gateway` | axum HTTP server |
| `bobaclaw-channel-telegram` | Telegram adapter |
| `bobaclaw-channel-web` | Local browser chat UI (`/ui`, `/api/web/*`, SSE) |
| `bobaclaw-scheduler` | Cron + delayed tasks |
| `bobaclaw-skills` | `SKILL.md` registry, guard, enable/disable |
| `bobaclaw-skill-forge` | draft-from-run → promote |
| `bobaclaw-mcp` | MCP hub (rmcp), prefixed tools |

### On-disk layout

```text
~/.bobaclaw/
├── config.yaml
├── state.db
├── runs/<run_id>/          # capsules, stdout/stderr
├── outbox/                 # CLI scheduled delivery
├── workspace/<group>/
│   ├── BOBACLAW.md, SOUL.md, MEMORY.md, TOOLS.md
│   ├── skills/, skills-staging/, memory/
│   └── inbox/telegram/...  # downloaded attachments
└── scheduler.pid           # daemon mode
```

---

## Feature list (implemented)

### CLI and operator

| Feature | Command / mechanism |
|---------|---------------------|
| Workspace init | `bobaclaw init` — config + seed from `workspace-examples/home` |
| Health check | `bobaclaw doctor` — API key, bwrap/docker, MCP, telegram, scheduler |
| Single message | `bobaclaw agent --message "..."` |
| REPL | `bobaclaw chat` — readline, history, markdown render |
| CLI slash commands | `/help`, `/new`, `/session`, `/compact`, `/stop`, `/subagents`, `/skills`, `/doctor`, `/quit` |
| Gateway | `bobaclaw gateway start` → `127.0.0.1:18790` |
| Web UI | `bobaclaw channel web start` → `127.0.0.1:18791/ui` (or `/ui` on the gateway) |
| Skills CLI | `list`, `view`, `enable`, `disable`, `drafts`, `guard`, `draft-from-run`, `promote` |
| Pairing | `pairing list/approve` |
| Schedule CLI | `schedule list/cancel` |
| Scheduler daemon | `scheduler start` (pidfile, Ctrl+C) |

### Gateway HTTP API

| Endpoint | Purpose |
|----------|---------|
| `GET /health` | Liveness |
| `POST /v1/chat/completions` | OpenAI-compatible (non-streaming) |
| `POST /api/agent` | `{ message, agent_group? }` |
| `POST /api/agent/interrupt` | Interrupt turn: `{ scope? (session:<id>), session_id?, agent_group? }` (default: group's REST/OpenAI sessions) |
| `GET /api/spawn/jobs?session_id=` | List background spawn jobs |
| `GET /api/spawn/jobs/{id}` | Spawn job details |
| `GET /ui`, `/api/web/*` | Web UI channel when `channels.web.enabled` (see below) |

Gateway also **automatically** starts Telegram long-poll and in-process scheduler when enabled in config.

**Auth:** every route except `GET /health` requires `Authorization: Bearer <token>` when a
token is configured (`gateway.auth_token_env`, default `BOBACLAW_GATEWAY_TOKEN`, or inline
`gateway.auth_token`); compared in constant time; 401 + `WWW-Authenticate: Bearer` otherwise.
Startup fails closed: a non-loopback `gateway.bind` without a token refuses to start; a
loopback bind without a token starts with a warning.

### Security posture

| Area | Behavior |
|------|----------|
| Sandbox env | bwrap `--clearenv` + fixed `PATH`/`HOME`/`LANG`/`TERM` (bwrap process itself also `env_clear()`); Docker `exec`/`create` forward no host env. Opt-in `executor.env_passthrough` for non-secret vars |
| Subagent CLI keys | Passed to the child only (bwrap: `0600` env file in a private temp dir, ro-bound at `/run/bobaclaw/secrets.env`, removed after the run; Docker: `exec -e NAME`). Never in command text, `script.sh`, `capsule.yaml`, logs, or host argv |
| Gateway HTTP | Bearer token (above); fail-closed on non-loopback binds |
| Docker deploy | Port published on `127.0.0.1` only; token required; gateway reaches Docker through `docker-socket-proxy` on an internal network (`DOCKER_HOST=tcp://docker-socket-proxy:2375`) with only PING/VERSION/INFO/CONTAINERS/EXEC/IMAGES/POST enabled — narrows the API surface, **not** a root boundary (container create + host binds remain possible) |
| Network | `executor.network: true` by default (compat); set `false` for untrusted input — see `harness/sandbox-contract.md` |

### Channels

**Telegram** (`bobaclaw channel telegram start` or via gateway):

- Long-poll, webhook cleanup
- DM policies: `pairing` / `allowlist` / `open`
- Group policies: `allowlist` / `open` / `disabled` + `group_require_mention`
- Pairing flow: `/pair`, `/start` → code → `bobaclaw pairing approve`
- Streaming UX: `editMessageText` at `stream_edit_interval_ms`
- Markdown → Telegram HTML (`format: html | plain`)
- Proxy (HTTP/SOCKS5) for Bot API
- Media into workspace: photo, document, voice, audio, video → `inbox/telegram/...`
- Slash: `/new`, `/stop`, `/subagents`, `/help`
- Bot commands registration (`setMyCommands`)

**CLI** — full channel with sessions and outbox for scheduled messages.

**Web UI** — see [Web UI channel](#web-ui-channel) below.

### Agent loop

- LLM ↔ tools loop up to `max_tool_iterations` (default 60)
- Nudges on empty replies (`max_action_retries`, `max_empty_response_retries`)
- **Serialization**: all turns on one session (user messages, spawn wakes, scheduled tasks) run one at a time
- **Interrupt / steering**: a new user message (CLI/chat/Telegram/Web UI) cancels the current turn on its session and older queued user messages; background ingress (spawn wake, cron, webhook, REST, OpenAI-compat) never cancels, it queues; `/stop`, Ctrl+C, `/api/agent/interrupt`
- Parallelism: `max_parallel_turns` (default 4) across different sessions; queued turns do not hold a slot
- Tool results persisted in history with `<!-- tool-results -->` marker
- Leaked tool XML filtered from model output
- System prompt: identity, agent loop, tool discipline, memory/skills/scheduling/subagent hints + workspace files (`BOBACLAW.md`, `SOUL.md`, `MEMORY.md`, skills index)

### Context / compaction

- Token estimation; proactive compaction before LLM call at `pre_call_compact_ratio: 0.8`
- LLM summarize of older messages (Hermes/OpenClaw pattern)
- Manual `/compact` in CLI
- Config: `context_window_tokens`, `reserve_tokens`, `keep_recent_messages`

### Tools (parent agent)

| Tool | Behavior |
|------|----------|
| `exec` | Shell in sandbox (bwrap/docker), run ledger, head/tail truncation |
| `schedule` | One-shot delayed task (up to 7 days) |
| `schedule_recurring` | Cron-style repeat (5-field cron) |
| `schedule_list` / `schedule_cancel` | Task management |
| `skill_manage` | create/patch/edit/delete/write_file skills |
| `skill_view` / `skills_list` | Read skills |
| `memory_manage` | append to `MEMORY.md` / `memory/*` |
| `subagent` | Synchronous delegation to isolated child loop |
| `spawn` | Fire-and-forget background subagent |
| `spawn_status` | Spawn job status |
| `mcp_<server>_<tool>` | Dynamic from MCP config |

**Child subagent** gets: `exec`, `skill_view`, `skills_list`, MCP (filtered by preset allowlist). No schedule, memory, or nested subagent tools.

### Subagents

| Backend | Status |
|---------|--------|
| `native` (default) | In-process tool loop, separate system prompt, semaphore concurrency |
| `claude_code` | CLI via sandbox (`claude --bare -p`) |
| `codex` | CLI via sandbox (`codex exec`) |
| `cursor` | Wrapper `scripts/cursor-subagent-wrapper.py` |

Config: `max_depth`, `max_concurrent`, presets (model, tools_allowlist, system_extra), spawn feedback (notify, wake parent, rate limit).

Spawn completion: notification to Telegram/CLI; optional wake of parent turn.

### Executor / sandbox

| Backend | Details |
|---------|---------|
| **bubblewrap** | Default on Linux; network toggle; `sandbox_packages` for apt |
| **Docker** | Default on macOS; image `bobaclaw/sandbox:latest`, named container |
| Run ledger | `runs` + `run_events` in DB; artifacts under `~/.bobaclaw/runs/` |
| Profiles | `bwrap-default`, networked, readonly; **`host-danger` — bail, not implemented** |

### Scheduler / automation

- Config cron jobs (`cron.jobs[]`) with delivery to Telegram/CLI
- Agent-created one-shot tasks (`scheduled_tasks` table)
- In-process scheduler in gateway + telegram (when `scheduler.enabled`)
- Embedded scheduler in `chat` (when `scheduler.embedded: true`)
- Foreground daemon with pidfile (`scheduler start`)
- Delivery: Telegram message or CLI outbox file

### Memory and skills

| Mechanism | Status |
|-----------|--------|
| Workspace markdown | `MEMORY.md`, `memory/`, injected into prompt |
| `memory_manage` tool | append only, size limits |
| Background memory review | every 10 user turns (async LLM) |
| Skills (agentskills.io) | `SKILL.md`, enable/disable, agent create via tool |
| Background skill review | after 10+ tool calls in one turn |
| Skill Forge | `draft-from-run` → staging → `promote` |
| Guard audit | `skills guard <path>` — static audit |

### MCP

- Transports: **stdio** (subprocess, incl. Docker Obscura) and **HTTP** (streamable)
- Prefixed tool names: `mcp_<server>_<tool>`
- Allowlist/denylist per server
- Docker MCP container cleanup on drop
- Doctor checks connectivity

### State DB (SQLite WAL)

| Table | Usage |
|-------|-------|
| `sessions`, `messages` | Dialog history |
| `messages_fts` | FTS5 + triggers (**schema only — no search API**) |
| `runs`, `run_events` | Run ledger |
| `approvals` | **Schema only — flow not implemented** |
| `routes` | channel+peer → agent_group+session |
| `pairing` | DM pairing codes |
| `cron_jobs`, `cron_runs` | Recurring automation |
| `scheduled_tasks` | One-shot agent schedules |
| `skill_drafts` | Skill Forge staging |
| `spawn_jobs` | Background subagent tracking |

### Provider

- **Single** OpenAI-compatible HTTP provider (`base_url`, `api_key_env`, `model`)
- Tool calling via `bobaclaw-provider`
- Subagents may use separate `subagents.model`
- **Not implemented:** failover, client-side streaming, Anthropic native, model routing

### Harness / CI

- Contracts in `harness/tools/` (exec, schedule, memory, skills, mcp, subagent)
- `make ci`, evals smoke, migration checks, integration scripts

---

## Scaffolding / not implemented

| Item | Where |
|------|-------|
| FTS search API / CLI | `memory_search` tool; no `bobaclaw search` CLI yet |
| Approval flow | `approvals` table, `host-danger` profile |
| `bobaclaw onboard` wizard | spec in ARCHITECTURE only |
| Second+ channel (Discord/Slack/…) | no crate |
| Built-in `web_search` | none |
| Built-in `web_fetch` | optional (`tools.web_fetch.enabled`, default off); MCP still primary for JS pages |
| Dedicated file tools (read/write/edit) | `file_read`, `file_write`, `file_edit` |
| Run output recall | `run_view` tool |
| Credential vault / proxy | keys in env/config; external subagent backends export keys into sandbox |
| Web UI / control panel | local chat UI only (`channels.web`); no admin/control panel |
| systemd unit / hot reload | none |
| Prometheus metrics | none |
| `bobaclaw migrate --from openclaw` | none |
| LLM streaming in gateway/CLI | non-streaming only |
| Multi-model failover | none |

---

## Positioning summary

BobaClaw is a **working self-hosted MVP** with full Claw DNA core:

- Gateway + CLI + Telegram
- Sandbox exec (bwrap/docker)
- SQLite sessions + run ledger
- Cron + agent scheduling
- MCP extensibility
- Skills + memory + background review
- Subagents (sync + async spawn)

Main gaps vs references (OpenClaw/Hermes/PicoClaw):

1. **Channel breadth** — one channel vs 6–20+ in references
2. **Operator UX** — no wizard, systemd; Web UI is chat-only
3. **Resilience** — single provider, no failover or streaming
4. **Security** — cleared sandbox env, gateway bearer auth, Docker API proxy; no vault, approvals, rate limiting, or host-danger
5. **Tool surface** — no built-in web/file/browser tools (MCP + exec instead)

---

## Web UI channel

**Crate:** `crates/bobaclaw-channel-web` · **Contract:** [harness/channels/web.md](../harness/channels/web.md) · **Off by default** (`channels.web.enabled: false`).

| Aspect | As built |
|--------|----------|
| Entry points | `/ui` mounted into `gateway start` (same axum `Router`, so gateway-level layers cover it); standalone `bobaclaw channel web start` on `channels.web.bind:port` (default `127.0.0.1:18791`) with `/health` |
| Frontend | One `include_str!` HTML page (`assets/index.html`), inline CSS/JS, no build step or CDN; sidebar of conversations, New chat, streaming turn view with collapsible tool blocks, Stop, safe markdown renderer, light/dark, mobile layout |
| API | `GET/POST /api/web/sessions`, `GET/POST /api/web/sessions/{id}/messages` (POST streams SSE), `POST /api/web/sessions/{id}/interrupt`, `GET /api/web/config` |
| Streaming | `AgentEvent` → `WebEvent` (`thinking`, `tool_start`, `tool_end`, `compacting`, `assistant_chunk`, `retry`, `interrupted`, `subagent_*`) + final `done` / `error`; unbounded mpsc from the progress callback into `axum::response::sse::Sse` |
| Sessions | `IngressKind::Web` (`source = web`), one session per conversation, explicit `session_id` on every turn; preempts in-flight turns like CLI/Telegram. New store methods: `create_session`, `get_session`, `list_sessions_for_ingress`, `list_history` |
| History | Assistant rows pass `sanitize_user_reply` (no `<!-- tool-results -->` appendix); `compaction` rows shown as collapsed summary markers |
| Auth | Bearer token from env `channels.web.auth_token_env` (default `BOBACLAW_GATEWAY_TOKEN`), constant-time compare, on all `/api/web/*`; `/ui` is public. No token + loopback → allowed with a warning; non-loopback bind without token → startup refused |
| Browser hardening | Nonce CSP (`default-src 'none'`, `connect-src 'self'`), `nosniff`, `X-Frame-Options: DENY`, no CORS; token only in `Authorization` header (stored in `localStorage`) |
| Doctor | `web ui: enabled=… standalone=… gateway=…` + token/bind check |
| Not implemented | Attachments, tool blocks in reloaded history, push of spawn/cron deliveries into the page, multi-group selection (uses `default_agent_group`) |

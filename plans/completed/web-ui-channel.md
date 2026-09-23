# Agent change plan: Web UI chat channel

## Goal

Let the operator chat with the agent from a browser: `http://127.0.0.1:18790/ui` (gateway) or `bobaclaw channel web start` (standalone), with multiple conversations, live turn progress over SSE, and Stop.

## Context

- Channels before this change: CLI/chat and Telegram. The gateway exposes REST/OpenAI-compat without a UI.
- `plans/backlog.md` listed "Web UI / control panel" as a non-goal; the operator explicitly requested a local chat UI. Scope is kept to chat only (no admin/control panel); the backlog note was narrowed accordingly.
- The dispatcher serializes turns by resolved `session_id` (`scope_gate.rs`) and preempts on interactive ingress (`IngressKind::preempts_in_flight`).
- A concurrent branch adds a bearer-auth layer to `crates/bobaclaw-gateway/src/server.rs`; the server change here is one line so the two merge cleanly, and web routes live inside the same `Router`.

## Scope

### In scope

- New crate `bobaclaw-channel-web` (router, auth, SSE events, embedded HTML).
- `IngressKind::Web` (interactive, preempts), `channels.web` config with defaults (off).
- `SessionStore` methods for listing/creating web sessions and reading history with timestamps.
- Gateway mount, `bobaclaw channel web start`, `bobaclaw doctor` check.
- Harness contract `harness/channels/web.md`, as-built section, README, config examples.

### Out of scope

- Attachments / uploads, multi-user accounts, CORS.
- Rendering tool blocks from stored history; push delivery of spawn/cron output into the page.
- Changes to executor, compaction, scope gate internals, subagent backends, `docker-compose.prod.yml`.

## Files changed

- `Cargo.toml`, `Cargo.lock` — workspace member + dep
- `crates/bobaclaw-channel-web/**` — new crate (`lib.rs`, `api.rs`, `auth.rs`, `events.rs`, `state.rs`, `assets/index.html`)
- `crates/bobaclaw-core/src/{request.rs,channels.rs,lib.rs}` — `IngressKind::Web`, `WebConfig`, `is_loopback_bind`
- `crates/bobaclaw-state/src/{session.rs,lib.rs}` — `create_session`, `get_session`, `list_sessions_for_ingress`, `list_history`
- `crates/bobaclaw-agent/src/{dispatcher.rs,lib.rs,channel_delivery.rs}` — `AgentDispatcher::pool()`, re-export `sanitize_user_reply`, `web` delivery → outbox
- `crates/bobaclaw-gateway/{Cargo.toml,src/server.rs}` — one-line `bobaclaw_channel_web::mount(...)`
- `crates/bobaclaw/{Cargo.toml,src/main.rs}` — `channel web start`, doctor
- `config.example.yaml`, `docker/config.docker.yaml`, `README.md`, `docs/as-built.md`, `harness/channels/web.md`, `plans/backlog.md`

## Implementation steps

1. Core: ingress kind + config + loopback helper, with unit tests.
2. State: session list/create/history methods, with tests.
3. Web crate: `TurnRunner` trait (implemented for `AgentDispatcher`, faked in tests), `WebState` with the SQLite pool, auth middleware, SSE handler (spawned turn + unbounded mpsc), HTML with per-request CSP nonce.
4. Wire gateway mount, CLI command, doctor.
5. Docs and config examples.

## Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace --no-fail-fast
make check-structure scan-secrets eval-smoke
```

Manual: temp `BOBACLAW_HOME`, `channel web start` and `gateway start` with web enabled; curl `/ui`, `/api/web/sessions` with/without token, create session, send a message against an unreachable provider (expect SSE `error`) and against a local mock provider (expect `tool_start`/`tool_end`/`done`); browser check of the page (token prompt on 401, streaming, tool block, Stop, mobile layout).

## Risks

- New network listener: mitigated by off-by-default, loopback default bind, refusal of non-loopback without a token, constant-time token check, strict CSP.
- Tokenless loopback mode lets any local process use the API (documented, logged as a warning, flagged by doctor).
- An abandoned browser stream does not cancel the turn (by design); the operator must use Stop.

## Rollback plan

Revert the branch commits. Without `channels.web.enabled` nothing is mounted, so disabling the key is an immediate operational rollback. No DB migration was added (new queries only).

## Completion notes

- changed files: as listed above.
- validation run: `cargo fmt --check` clean; clippy shows no warnings in touched files (pre-existing warnings in `bobaclaw-agent` remain); `cargo test --workspace --no-fail-fast`: 192 passed, 2 failed (pre-existing `bobaclaw-executor` `docker_mount` tests on macOS); new tests: 18 in `bobaclaw-channel-web`, 3 in `bobaclaw-state`, 3 in `bobaclaw-core`. `make check-structure scan-secrets eval-smoke` OK. Real-binary checks passed as described in Validation.
- known gaps: no attachments; reloaded history shows final text only (no tool blocks); web sessions always use `default_agent_group`; switching conversations mid-turn detaches the stream (turn continues server-side, visible after reload); scheduled-task delivery from a web turn falls back to the CLI outbox.
- follow-up work: optional per-session "busy" indicator, tool blocks from the run ledger in history, agent-group picker.

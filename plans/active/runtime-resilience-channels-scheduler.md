# Agent change plan

## Goal

Make the long-running runtime degrade predictably: no lost Telegram messages, cron catch-up after downtime, automatic recovery of the polling task, provider failover, and MCP binding refresh after reconnect.

## Context

Priority **P1 (reliability core)** — part of the June 2026 reliability/autonomy review roadmap. Depends on nothing; pairs well with `executor-timeout-and-cancel-kill.md`.

Findings:

- Telegram offset is advanced in memory **before** the user message is persisted; the turn runs in a fire-and-forget `tokio::spawn` (`crates/bobaclaw-channel-telegram/src/runtime.rs`). A crash in that window loses the message; restart resets offset to 0 and may duplicate updates.
- The Telegram polling task ends permanently on fatal error; gateway keeps running with no supervisor (`crates/bobaclaw-gateway/src/server.rs:73-76`).
- Cron lookback window is `tick_secs + 2` (~17s with defaults) (`crates/bobaclaw-scheduler/src/runner.rs:269-270`): all fires missed during downtime are silently skipped. Tasks stuck in `running` after a crash are never reset. One-shot failures are terminal with no retry.
- Single LLM provider; retries exist (3x, backoff) but no secondary model/endpoint failover (`crates/bobaclaw-provider/src/tools_chat.rs`).
- MCP reconnect retries the tool call but does not re-list tools into `bindings` (`crates/bobaclaw-mcp/src/hub.rs:158-164`); servers that fail at startup are lost until gateway restart.
- SQLite pools have no explicit `busy_timeout`; gateway + standalone scheduler + CLI can hit `SQLITE_BUSY` under write contention (`crates/bobaclaw-state/src/db.rs`).

## Scope

### In scope

- Persist Telegram `update_id` in SQLite; advance it only after the inbound message row is durably written; dedupe on redelivery.
- Supervisor loop for the Telegram polling task in gateway: restart with exponential backoff, log + health signal on repeated failure.
- Cron misfire policy: configurable `catch_up: skip | run_once` (default `run_once` for one fire per missed job); reset stale `running` scheduled tasks to `pending` (or `failed` with note) on scheduler startup.
- Optional retry budget for one-shot scheduled tasks (`max_attempts`, default 1 to preserve behavior).
- Provider failover: optional `provider.fallback` (base_url/model/api_key_env); switch after the primary retry budget is exhausted on retryable errors.
- MCP: refresh tool bindings after reconnect; optional periodic re-connect attempt for servers that failed at startup.
- Add `busy_timeout` pragma to SQLite connections.

### Out of scope

- Webhook mode for Telegram (long-poll stays).
- Full durable outbox with delivery acks (current scheduler outbox file drop unchanged here).
- New channels.

## Files likely to change

- `crates/bobaclaw-channel-telegram/src/runtime.rs`
- `crates/bobaclaw-state/src/db.rs` + new migration (`channel_offsets` or similar)
- `crates/bobaclaw-gateway/src/server.rs`
- `crates/bobaclaw-scheduler/src/runner.rs`
- `crates/bobaclaw-provider/src/tools_chat.rs`, `crates/bobaclaw-provider/src/config.rs`
- `crates/bobaclaw-mcp/src/hub.rs`
- `crates/bobaclaw-core/src/config.rs`
- `config.example.yaml`
- `migrations/`

## Implementation steps

1. Migration + state API for persistent channel offsets; rewire poll loop ordering (persist message → advance offset); dedupe by `(chat_id, message_id)`.
2. Wrap Telegram polling in a supervised restart loop with backoff in gateway.
3. Scheduler: startup recovery of stale `running` rows; cron catch-up window/policy; optional one-shot retries.
4. Provider fallback config + failover path in `chat_turn`; surface which provider answered in tracing.
5. MCP reconnect re-lists tools and updates bindings; periodic retry for dead servers.
6. `busy_timeout` pragma; concurrency smoke test (gateway + scheduler + CLI writing simultaneously).

## Validation

```bash
make ci
cargo test -p bobaclaw-channel-telegram -p bobaclaw-scheduler -p bobaclaw-provider -p bobaclaw-mcp -p bobaclaw-state
```

Additional checks:

- Manual: kill gateway mid-turn, restart, verify the inbound message is processed exactly once.
- Manual: stop scheduler for > 2 cron intervals, restart, verify one catch-up fire per job with `run_once`.

## Risks

- Offset persistence changes the poll hot path; bugs here can cause duplicate turns — dedupe table mitigates.
- `run_once` catch-up may fire stale prompts after long downtime; per-job `catch_up: skip` covers noisy jobs.
- Provider failover can mask primary misconfiguration; log loudly on every failover.

## Rollback plan

Revert the branch. The offsets migration is additive; on rollback the poll loop simply returns to in-memory offsets.

## Completion notes

Fill this after implementation:

- changed files:
- validation run:
- known gaps:
- follow-up work:

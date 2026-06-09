# Async spawn feedback integration

## Goal

Durable `spawn_jobs` store with multi-channel delivery (session + notify + parent wake), `spawn_status` tool, and gateway HTTP API — replacing in-memory `spawn_tasks`.

## Completion notes

Implemented per attached plan (`async_spawn_feedback_a3970ac6`).

### Changed areas

- Migration `migrations/20260609100000_spawn_jobs.sql` + `SpawnJobStore` in `bobaclaw-state`
- DB-backed `spawn_async` / `run_sync` link + `SpawnCompleter` wake/notify
- `ChannelDelivery` trait (`cli` outbox, `telegram` sendMessage)
- `spawn_status` parent tool; `/subagents` (CLI + Telegram) + gateway `GET /api/spawn/jobs`
- `IngressKind::SpawnWake`, `subagents.spawn` config in `config.example.yaml`
- Harness docs: `harness/tools/subagent.md`, `harness/channels/telegram.md`

### Validation

```bash
make fmt-check          # OK
cargo build --workspace # OK
cargo test -p bobaclaw-state spawn
cargo test -p bobaclaw-agent spawn
cargo test -p bobaclaw-channel-telegram
```

`make ci` fails on pre-existing `bobaclaw-executor` `docker_mount` tests on macOS (`/private/var` vs `/var`); unrelated to this change.

### Known gaps

- `job_retention_days` config present; cleanup job not implemented
- `POST /api/spawn/jobs/:id/cancel` deferred
- No dedicated gateway route integration tests (handlers are thin DB passthrough)

### Rollback

Revert migration + code; disable via `subagents.spawn.wake_parent_on_complete: false` and `notify_on_complete: false`, or `subagents.enabled: false`.

# Agent change plan: Subagent subsystem fixes

## Goal

Fix cancel propagation, TurnContext run-id chain, ledger finalization, and external backend timeout floor discussed in subagent review.

## Completion notes

### Changed files

- `crates/bobaclaw-agent/src/turn_context.rs` — `run_id`, `for_delegation()`, fixed `child()` parent link
- `crates/bobaclaw-agent/src/subagent/mod.rs` — cancel via `child_token()`, ledger `finalize_subagent_ledger`, child `run_id`
- `crates/bobaclaw-agent/src/tools/spawn.rs` — pass parent cancel
- `crates/bobaclaw-agent/src/tool_loop.rs` — delegation context with `last_run_id`
- `crates/bobaclaw-agent/src/subagent/backends/mod.rs` — timeout floor `max(1)` not `max(60)`

### Validation

```text
cargo test -p bobaclaw-agent — 53 passed
cargo test -p bobaclaw-channel-telegram — passed
make fmt-check — passed
```

### Known gaps

- Spawn task list remains in-memory only (Phase D follow-up)
- External backends still create a separate CLI ledger run in addition to `subagent_*` row

### Rollback

Revert the files above; set `subagents.enabled: false` disables tools without code rollback.

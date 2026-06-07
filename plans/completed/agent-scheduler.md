# Agent-managed scheduler and in-process runner

## Goal

Let the runtime agent create one-shot and recurring scheduled tasks without manual cron/config edits, and fix scheduler not running when only `channel telegram start` is used.

## Context

User reported: (1) recurring tasks required hand-written `config.yaml` cron; (2) one-shot reminder ("пиво in 30s") never delivered. Root cause: scheduler only ran with `embedded: true` or separate `bobaclaw scheduler start` daemon; `deliver_message` still invoked LLM at fire time.

## Scope

### In scope

- Agent tools: `schedule_recurring`, `schedule_list`, `schedule_cancel`
- In-process scheduler for gateway and `channel telegram start`
- Adaptive scheduler sleep for soon-due one-shots
- Skip LLM when `deliver_message` is set
- DB migration for cron delivery fields
- Harness contract, policy, plan, docs updates

### Out of scope

- Model-based eval runner for scheduler
- systemd unit changes
- Windows pidfile/process detection

## Files changed

- `crates/bobaclaw-agent/src/tools/schedule.rs`, `turn.rs`, `prompt.rs`
- `crates/bobaclaw-scheduler/src/runner.rs`, `lib.rs`
- `crates/bobaclaw-gateway/src/server.rs`
- `crates/bobaclaw/src/main.rs`, `interactive.rs`
- `crates/bobaclaw-state/src/cron.rs`, `scheduled.rs`
- `migrations/20260607100000_cron_delivery.sql`
- `config.example.yaml`
- `harness/tools/schedule.md`, `harness/policy.md`
- `docs/features.md`

## Implementation steps

1. Add agent cron CRUD tools and extend `CronStore` with delivery columns.
2. Spawn scheduler in gateway/channel; adaptive tick; direct `deliver_message` delivery.
3. Update harness contracts, policy, plan, and feature docs.
4. Run `cargo test -p bobaclaw-agent` and `make ci`.

## Validation

```bash
cargo test -p bobaclaw-agent schedule::
cargo test -p bobaclaw-state
cargo build -p bobaclaw
make ci
```

## Risks

- Agent-created cron jobs persist until cancelled — operator should use `schedule_list` / DB for audit.
- Cron without deliver target (legacy config jobs) still runs agent turn but may not notify user.

## Rollback plan

Revert branch `feature/agent-scheduler`; migration columns are additive (safe to leave). Disable `scheduler.enabled` in config if needed.

## Completion notes

- changed files: see scope above; branch `feature/agent-scheduler`, commit `e91857f` (+ harness/docs follow-up)
- validation run: `cargo test -p bobaclaw-agent` (28 passed), `cargo build -p bobaclaw` OK
- known gaps: no `evals/regression/` scenario for scheduler delivery yet; Windows daemon pidfile still no-op
- follow-up work: add regression eval for 30s Telegram reminder; optional `bobaclaw cron list` CLI

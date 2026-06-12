# Agent change plan: F7 — Event triggers (minimal proactivity)

## Goal

Deliver cheap proactivity via authenticated webhook and file watcher that enqueue agent turns — minimal precursor to F12 Watcher (do not over-engineer).

## Context

- F7 webhook and file watcher are **later refactored** into Watcher `PushSource`/`PollSource` (F12).
- Payloads are untrusted (F5).
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

1. **Webhook:** `POST /api/events/{source}` (bearer token per source) → enqueue turn for mapped `agent_group` with templated prompt.
   - Config: `events.sources.<name>.{token, agent_group, prompt_template}`.
   - Payload as `{{payload}}` (size-capped, tainted untrusted).

2. **File watcher:** `events.watch: [{path, glob, agent_group, prompt_template, debounce_ms}]` using `notify` crate inside scheduler daemon/gateway.

3. **Dedup/rate limit:** `min_interval_seconds` per source.

4. **Delivery:** reuse existing outbound paths (Telegram / CLI outbox).

### Out of scope

- Rules engine, scoring, digest batching (F12).
- New notification tiers (F16).

## Files likely to change

- `crates/bobaclaw-gateway/` — webhook route
- `crates/bobaclaw-scheduler/` or gateway in-process — file watcher
- `crates/bobaclaw-core/src/config.rs` — `events` block
- `crates/bobaclaw-agent/` — turn enqueue from external trigger
- `harness/tools/event-triggers.md`
- `docs/as-built.md`

## Implementation steps

1. Config schema with defaults (off by default).
2. Bearer auth per source on webhook.
3. Payload cap + taint tag + template render.
4. Enqueue turn via existing dispatcher path.
5. File watcher with debounce and glob filter.
6. Per-source rate limit / dedup.
7. Doctor: webhook token configured, watch paths exist.
8. Harness: webhook → turn → Telegram mock reply; watcher temp dir test.

## Validation

```bash
make ci
```

## Risks

- Webhook abuse if token leaked — off by default, doctor warns.
- Watch path symlinks escaping workspace — restrict to configured roots.

## Rollback plan

- Disable `events` in config; remove routes.

## Dependencies

- F5 recommended for payload taint (can tag all webhook payload untrusted without full F5).

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work: refactor into F12 `PushSource`/`PollSource`

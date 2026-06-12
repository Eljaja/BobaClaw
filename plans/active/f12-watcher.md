# Agent change plan: F12 — Watcher (event ingestion, triage, agent wake-up)

## Goal

Turn BobaClaw proactive via a dedicated watcher pipeline: ingest events from RSS/GitHub/webhook/page-diff sources, apply cheap rules (optional budgeted LLM score), and wake or digest — without an observation daemon.

## Context

- **Hard dependency: F5 (taint)** — all Watcher content is untrusted; must not trigger side-effectful tools without approval.
- **Soft dependencies:** F4 (budget scope), F3 (metrics), F7 (webhook/file watcher refactored into sources).
- F7 is minimal precursor; refactor F7 endpoints into `PushSource`.
- **Baseline:** `docs/as-built.md`.

## Architecture

```text
Source → Normalize(Event) → Dedup → Rules (cheap, no LLM) → Score (optional, budgeted LLM) → Outcome
Outcome ∈ { wake (turn now) | digest (batch, scheduled) | drop }
```

New crate `bobaclaw-watcher`. Runs inside gateway and/or scheduler daemon (config flag), same pattern as in-process scheduler.

### Core types

```rust
struct Event {
    source: String,
    kind: String,
    external_id: String,
    title: String,
    body: String,          // size-capped; always tainted untrusted
    url: Option<String>,
    ts: DateTime<Utc>,
    raw: serde_json::Value // capped
}

trait PollSource  { async fn poll(&self, cursor: Option<Cursor>) -> Result<(Vec<Event>, Cursor)>; }
trait PushSource  { /* registered HTTP route or channel; emits Events */ }
```

### Persistence

| Table | Essentials |
|-------|------------|
| `watch_sources` | id, name, kind, config_json, cursor, enabled, last_poll_ts, last_error, consecutive_failures |
| `watch_events` | id, source, external_id (UNIQUE with source), kind, title, body, url, ts, outcome, score, rule_matched |
| `watch_digests` | id, agent_group, schedule, last_delivered_ts |

### Config (off by default)

See full `watcher:` block in backlog spec: sources (rss, github_*, osv_advisories, webhook, page_diff), rules engine, optional score stage, digest cron, wake rate limits.

### v1 connectors (exactly four)

| Kind | Notes |
|------|-------|
| `rss` | ETag/Last-Modified; cursor = last item id/ts |
| `github_releases`, `github_issues`, `github_advisories`, `osv_advisories` | Token via `{{secret:}}` (F2) |
| `webhook` | Refactor F7 into `PushSource` |
| `page_diff` | fetch + optional CSS selector + content hash; body = unified diff |

### Rules engine (main investment)

- Predicates: `eq`, `neq`, `gte`/`lte`, `regex`, `contains`, `in`, `match: always`; combinators `all`/`any`.
- Fields: dotted paths into Event + `raw`.
- First matching rule wins; implicit final = `drop`.
- Outcome `score` → optional LLM scorer → wake/digest/drop via thresholds.

### Integration requirements

- **Taint (F5):** every Event body/title untrusted; templates use delimited blocks; injection policy on wake turn.
- **Budgets (F4):** scorer and wake-turns from `budget_scope: watcher`; breach demotes wake → digest.
- **Metrics (F3):** `bobaclaw_watch_events_total{source,outcome}`, `bobaclaw_watch_poll_errors_total{source}`, `bobaclaw_watch_wake_turns_total`.
- **Scheduler:** digest delivery via existing cron.
- **Workspace:** `WATCH.md` injected into scorer; `watch_manage` tool (append notes, enable/disable sources — no agent CRUD on config in v1).
- **CLI:** `bobaclaw watch list|test <source>|events [--since --outcome]`; `watch test` = dry run.
- **Doctor:** per-source connectivity; stale cursor warning (3× interval).
- **Failure policy:** exponential backoff; auto-disable after N failures + notify.

### v1 non-goals

- Semantic/embedding dedup.
- IMAP, calendar, screen observation.
- Agent-managed source CRUD.
- Per-event vector storage.

## Files likely to change

- New: `crates/bobaclaw-watcher/`
- `migrations/` — watch_* tables
- `crates/bobaclaw-gateway/`, `crates/bobaclaw-scheduler/`
- `crates/bobaclaw-agent/` — wake/digest turn enqueue, `watch_manage` tool
- `crates/bobaclaw-core/src/config.rs`
- Refactor F7 webhook into `PushSource`
- `harness/tools/watcher.md` — table-driven rules tests
- `docs/as-built.md`

## Implementation steps

1. Crate scaffold + Event types + migrations.
2. Rules engine with table-driven unit tests.
3. RSS `PollSource` with cursor dedup.
4. GitHub/OSV connectors.
5. Webhook `PushSource` (from F7).
6. `page_diff` connector.
7. Poll loop in gateway/scheduler with backoff.
8. Wake path → dispatcher + rate limit + overflow to digest.
9. Digest cron aggregation + template render.
10. Optional LLM scorer (F4 budget).
11. `watch_manage`, `WATCH.md`, CLI, doctor.
12. Full acceptance suite (6 scenarios in spec).

## Validation

```bash
make ci
```

Acceptance:
1. RSS fixture: two polls, no duplicate events, cursor advances.
2. Rules harness: fixture events → outcomes incl. combinators and implicit drop.
3. Rate limit: 10 wakes/hour, max 6 → 6 turns + 4 demoted to digest.
4. Taint: RSS injection text → wake turn exec gated by approval.
5. Digest: 5 events → one scheduled turn with all in `{{events}}`.
6. `watch test` dry run: no DB writes, no turns.

## Risks

- Connector scope creep — stick to four kinds.
- Notification fatigue without F16 triage.

## Rollback plan

- `watcher.enabled: false`.

## Dependencies

- **F5 required** (taint + injection policy on wake turns).
- F4, F3, F7 soft; F14 for approval provenance from watcher events.

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work: F16 notification triage; F14 watcher trace in approvals

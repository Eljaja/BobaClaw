# Agent change plan: F14 — Provenance-carrying approvals

## Goal

Extend F1 approvals with causal trace (ingress, untrusted sources, tool args, model rationale) in Telegram/CLI/TUI and immutable audit log.

## Context

- Sensitive-operation confirmation must show **why** — not just "approve exec? y/n".
- Composes on F1; enriched when F5 (taint) and F12 (watcher) exist.
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

1. **Causal trace on each approval request:**
   - Originating ingress (who/what started the turn).
   - Whether untrusted content (F5) is in causal path and from which source/event (F12).
   - Tool + concrete args.
   - One-line model-stated rationale (if available from turn state).

2. **Compact rendering**, e.g.:
   `⚠ exec (high) — rm -rf ./cache · triggered by: RSS event "build failed" (untrusted) · group: home`

3. **Audit table:** `audit_log(ts, actor, action, target, decision, trace_json)` — immutable payloads logged with decision + operator identity.

4. Surfaces: Telegram, CLI, gateway API response bodies, later F11 TUI.

### Out of scope

- Full distributed tracing UI (F3 covers OTel).

## Files likely to change

- `migrations/` — `audit_log`
- `crates/bobaclaw-agent/` — build trace at approval creation
- `crates/bobaclaw-state/`
- `crates/bobaclaw-channel-telegram/` — formatted message
- `crates/bobaclaw-gateway/` — API includes trace
- `crates/bobaclaw/` — CLI display
- `harness/tools/provenance-approvals.md`
- `docs/as-built.md`

## Implementation steps

1. Define `ApprovalTrace` struct (serde, stable schema).
2. Capture ingress + taint path during turn (hook F5/F12 metadata).
3. Store trace in `approvals` payload + `audit_log` on decision.
4. Render templates for Telegram/CLI.
5. Gateway list/detail includes trace_json.
6. Acceptance: watcher-triggered approval shows untrusted source; decision persisted in audit_log.

## Validation

```bash
make ci
```

## Risks

- Trace too verbose for Telegram message limits — truncate with "details in CLI/gateway".
- Missing rationale when model didn't state one — omit field gracefully.

## Rollback plan

- Fall back to F1 minimal approve/deny UI; audit_log optional.

## Dependencies

- **F1** required.
- **F5** for untrusted path marking.
- **F12** for watcher event attribution (can stub until F12).

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work:

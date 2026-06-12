# Agent change plan: F11 — Minimal TUI (`bobaclaw top`)

## Goal

Ratatui-based operator dashboard reading gateway API only: live turns, pending approvals, spawn jobs, today's usage — no new state paths.

## Context

- Web UI explicitly deferred; TUI covers operator loop (F1 approvals, F3/F4 observability data via API).
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

- `bobaclaw top` (ratatui):
  - Active/recent turns (from gateway if exposed or derived).
  - Pending approvals with inline approve/deny (F1 API).
  - Spawn job list/status.
  - Today's usage summary (F4 API).
- Reads gateway HTTP only (`127.0.0.1:18790` default).
- Graceful degrade if gateway down.

### Out of scope

- Full chat TUI (existing `chat` REPL remains).
- WebSocket streaming (poll OK for v1).

## Files likely to change

- `crates/bobaclaw/` — `top` subcommand + ratatui dependency
- `crates/bobaclaw-gateway/` — may need thin aggregate endpoints if missing
- `docs/as-built.md`

## Implementation steps

1. Gateway client helpers for approvals, spawn, usage endpoints.
2. Ratatui layout: header, turns pane, approvals pane, footer keys.
3. Keybindings: approve/deny selected approval, quit, refresh.
4. Poll loop with configurable interval.
5. Document terminal requirements.

## Validation

```bash
make ci
# Manual: bobaclaw top against running gateway
```

## Risks

- Terminal size / SSH — minimum cols/rows check.
- API gaps — add minimal gateway routes without duplicating state.

## Rollback plan

- Optional feature flag or separate binary feature `tui`.

## Dependencies

- F1 approvals API.
- F4 usage API (or stub section).
- F3 not required in TUI v1.

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work:

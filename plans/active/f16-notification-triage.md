# Agent change plan: F16 — Silent-failure detection & notification triage

## Goal

Improve reliability of long-running and proactive operation: turn-end reminders for open commitments, stall watchdog, unified outbound notification tiers — so Watcher is not muted and silent failures are caught.

## Context

- Proactive agents (F12) fail UX-wise when they over-notify or silently abandon work.
- Composes F12 (route wake/digest through triage), F15 (stall halt), F4 (optional checker budget).
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

1. **System reminders (`reminders.enabled`):**
   - Turn ends with open commitment (declared plan/TODO unfinished, unaddressed tool error) → inject targeted reminder, resume loop instead of silent end.
   - Lightweight turn-end checks only.

2. **Heartbeat / liveness:**
   - Long/scheduled turns emit progress to run ledger.
   - Watchdog halts + notifies on stall beyond `max_turn_wall_clock` (composes F15).

3. **Notification triage (proactive UX):**
   - Unified outbound layer with tiers: `critical` (interrupt now), `normal` (batch), `low` (digest only).
   - Correlate related events into one message; quiet hours; route F12 wake/digest through this layer.
   - Overflow demotes, never silently drops.

   ```yaml
   notifications:
     tiers: ...
     quiet_hours: ...
     batch_window: ...
   ```

4. **Optional second-agent validation (off by default):**
   - High-stakes autonomous outputs → cheap checker turn before act/notify.
   - Budgeted via F4 `review` or dedicated scope.

### Out of scope

- Full mobile push infrastructure.
- User-editable ML relevance model.

## Files likely to change

- `crates/bobaclaw-agent/src/turn.rs` — turn-end checks, reminders
- New or extend: outbound notification module (core or agent)
- `crates/bobaclaw-channel-telegram/` — tier-aware delivery
- `crates/bobaclaw-scheduler/` — digest batch windows
- `crates/bobaclaw-watcher/` — integrate triage (after F12)
- `crates/bobaclaw-core/src/config.rs`
- `harness/tools/notification-triage.md`
- `docs/as-built.md`

## Implementation steps

1. Turn-end commitment detector (heuristics: TODO in assistant text, failed tool without follow-up).
2. Reminder injection + limited re-loop.
3. Progress events in run ledger for long turns.
4. Stall watchdog task + F15 integration.
5. Notification tier enum + batch queue + quiet hours.
6. Correlation key (source, group, thread) for batching.
7. Refactor F12/Telegram/cron delivery to use triage layer.
8. Optional checker turn hook (config off by default).
9. Acceptance tests (3 scenarios in spec).

## Validation

```bash
make ci
```

Acceptance:
1. Turn says "I'll schedule X" but no task → reminder → task created.
2. Five related watcher events in batch window → one notification; critical interrupts immediately.
3. Stalled scheduled turn halted with operator notice.

## Risks

- Reminder loops if detector too aggressive — cap re-loop count.
- Quiet hours delaying critical alerts — tier override rules.

## Rollback plan

- Disable `reminders`, `notifications` blocks; direct delivery like today.

## Dependencies

- F12 for watcher routing through triage.
- F15 for stall halt.
- F4 for checker budget.

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work:

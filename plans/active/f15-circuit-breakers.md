# Agent change plan: F15 — Circuit breakers & anomaly halting

## Goal

Halt runaway turns and isolate failures per scope: repeat tool loops, error storms, egress anomalies, cost-velocity spikes — with operator notification and optional approval to resume.

## Context

- Agentic chains (loops, subagents, schedules, F12 wakes) can cascade into compromise or runaway cost.
- Composes F1 (resume approval), F4 (cost velocity), F5 (egress under wrong profile).
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

1. **Runaway breakers** — trip on:
   - N identical/near-identical tool calls in one turn;
   - tool-error rate over threshold;
   - exec network egress under non-networked profile;
   - cost-velocity spike (F4).

   On trip → halt turn, notify operator, require approval to resume (if `on_trip: approve`).

2. **Per-scope isolation:** tripped breaker for one `agent_group`/source must not stall others (extend dispatcher, don't regress serial-per-scope).

3. **Egress anomaly hook (opt-in):** in networked profiles, log outbound destinations; flag hosts not previously seen for that group. Pairs with F2 redaction.

4. **Config:**
   ```yaml
   circuit_breakers:
     enabled: false
     repeat_call_limit: 5
     tool_error_rate: 0.5
     cost_velocity_usd_per_min: 1.0
     on_trip: halt    # halt | approve
   ```

5. **Metric:** `bobaclaw_breaker_trips_total{kind}` (F3).

### Out of scope

- Global process kill / gateway shutdown.
- ML anomaly detection.

## Files likely to change

- `crates/bobaclaw-agent/src/turn.rs` — breaker checks in tool loop
- `crates/bobaclaw-executor/` — egress logging hook
- `crates/bobaclaw-core/src/config.rs`
- `crates/bobaclaw-state/` — per-group egress history (if persisted)
- `harness/tools/circuit-breakers.md`
- `docs/as-built.md`

## Implementation steps

1. Breaker state machine per turn.
2. Repeat-call fingerprint (tool name + normalized args hash).
3. Error rate window in turn.
4. Egress profile violation check.
5. Cost velocity from F4 ledger (rolling window).
6. Trip handler: halt + notify + metric.
7. Resume via F1 approval when configured.
8. Scope isolation verification test.
9. Acceptance: looping exec trips breaker; second group unaffected.

## Validation

```bash
make ci
```

## Risks

- Legitimate retry loops (e.g. polling) → tune limits per tool or config overrides.
- Egress log noise on CDN-heavy workloads.

## Rollback plan

- `circuit_breakers.enabled: false`.

## Dependencies

- F1 for approve-to-resume.
- F4 for cost velocity.
- F3 for metric.

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work: F16 watchdog composes with `max_turn_wall_clock`

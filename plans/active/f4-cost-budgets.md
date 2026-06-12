# Agent change plan: F4 — Cost accounting & budgets

## Goal

Track per-LLM-call token usage and USD cost, enforce configurable budgets before LLM calls, and expose usage via CLI.

## Context

- Provider returns usage in API responses; run ledger exists but no cost ledger.
- Background review loops can silently burn tokens.
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

1. **Persistence:** per LLM call record `model, prompt_tokens, completion_tokens, cost_usd`.
   - Price table: `pricing.models.<name>.{input_per_mtok, output_per_mtok}`.
   - New table `usage_ledger(id, ts, session_id, agent_group, scope, model, prompt_tokens, completion_tokens, cost_usd, source)`.
   - `source` ∈ `turn|subagent|cron|review`.

2. **Config:**
   ```yaml
   budgets:
     default: { daily_usd: 5.0 }
     agent_groups:
       home: { daily_usd: 2.0, per_turn_usd: 0.5 }
     cron: { daily_usd: 1.0 }
   ```

3. **Enforcement:** check before each LLM call; on breach → abort turn with operator-visible message, counter metric (F3), optional Telegram notify. Subagents inherit parent budget scope.

4. **CLI:** `bobaclaw usage [--group --since --by model|group|source]` — table output.

5. **Review budget:** memory/skill background reviews count against separate `review` scope.

### Out of scope

- Billing provider integration.
- Hard multi-tenant isolation.

## Files likely to change

- `migrations/` — `usage_ledger`
- `crates/bobaclaw-state/`
- `crates/bobaclaw-provider/` — capture usage from responses
- `crates/bobaclaw-agent/src/turn.rs` — pre-call budget check
- `crates/bobaclaw-agent/src/review.rs` — review scope accounting
- `crates/bobaclaw-core/src/config.rs`
- `crates/bobaclaw/` — `usage` subcommand
- `harness/tools/budgets.md`
- `docs/as-built.md`

## Implementation steps

1. Migration for `usage_ledger`.
2. Pricing config parsing and cost calculation helper.
3. Insert ledger row after each LLM call (all sources).
4. Budget aggregation queries (daily, per-turn).
5. Pre-call enforcement in main turn loop.
6. Subagent inherits parent scope.
7. Cron/review tagged with correct `source`.
8. Abort path with user-visible message + metric.
9. CLI `usage` with grouping flags.
10. Harness: fake provider with fixed tokens → budget breach aborts mid-turn; CLI sums match ledger.

## Validation

```bash
make ci
```

## Risks

- Missing pricing for model → zero cost or hard fail (document choice).
- Clock skew on daily window boundaries.

## Rollback plan

- Omit budget checks if config section absent; ledger can remain for reporting.

## Dependencies

- Soft: F3 for breach metric.

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work: F12 watcher `budget_scope`; F15 cost-velocity breaker

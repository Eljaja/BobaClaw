# Agent change plan: Hermes memory-first persistence split

## Goal

Rebalance BobaClaw persistence so facts go to workspace memory and only repeatable workflows become skills, matching Hermes's dual post-turn review model.

## Context

BobaClaw imported Hermes skill review (background LLM + forge auto-promote) but not memory review. The prompt nudged `skill_manage` after 5+ tool calls, causing over-skilling. ADR 004 requires manual skill promotion in v1.

## Scope

### In scope

- `memory_manage` tool (append-only, workspace-scoped)
- Background memory review every 10 user turns
- Tightened skill review; remove forge auto-promote fallback
- Prompt and workspace template updates
- Harness contract and unit tests

### Out of scope

- MemoryManager prefetch/sync plugins
- FTS search API
- Config.yaml thresholds
- Skill Forge cron automation

## Files likely to change

- `crates/bobaclaw-agent/src/tools/memory.rs`
- `crates/bobaclaw-agent/src/tools/mod.rs`
- `crates/bobaclaw-agent/src/review.rs`
- `crates/bobaclaw-agent/src/loop_.rs`
- `crates/bobaclaw-agent/src/turn.rs`
- `crates/bobaclaw-agent/src/prompt.rs`
- `crates/bobaclaw-state/src/session.rs`
- `harness/tools/memory.md`
- `workspace-examples/home/BOBACLAW.md`

## Implementation steps

1. Add `memory_manage` tool and harness contract.
2. Add `count_user_messages` to SessionStore.
3. Refactor `review.rs` with memory + skill tracks and `maybe_post_turn_review`.
4. Wire post-turn orchestrator in `loop_.rs`; track `memory_manage_used` in `turn.rs`.
5. Update prompt hints and workspace template.
6. Add tests; run `make ci`.

## Validation

```bash
cargo test -p bobaclaw-agent
cargo test -p bobaclaw-state
make ci
```

## Risks

- Extra background LLM calls when gates fire — mitigated by independent thresholds.
- Memory file growth — mitigated by append size caps.

## Rollback plan

Revert `review.rs` orchestrator, remove `memory_manage` from tool list, restore original `SKILLS_HINT` and forge fallback.

## Completion notes

- changed files:
  - `crates/bobaclaw-agent/src/tools/memory.rs`, `tools/mod.rs`, `review.rs`, `loop_.rs`, `turn.rs`, `prompt.rs`
  - `crates/bobaclaw-state/src/session.rs`
  - `crates/bobaclaw-skill-forge/src/forge.rs` (`draft_and_promote_from_run` marked `dead_code` per ADR 004)
  - `harness/tools/memory.md`
  - `workspace-examples/home/BOBACLAW.md`
  - `plans/completed/hermes-memory-skill-split.md`
- validation run: `cargo test -p bobaclaw-agent` (40 passed), `cargo test -p bobaclaw-state` (4 passed), `make ci` (exit 0)
- known gaps: no MemoryManager prefetch/sync; review thresholds are constants (not `config.yaml`); background reviews require a live LLM API key
- follow-up work: `bobaclaw search` FTS API; optional config thresholds for memory/skill review intervals

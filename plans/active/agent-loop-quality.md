# Agent change plan

## Goal

Sharpen the agent loop's internal signals: real token accounting, action-aware nudges, parallel read-only tool batches, staged compaction, cancellable compaction, cheap-model routing for auxiliary calls, and surfacing background review results.

## Context

Priority **P2 (loop quality)** — part of the June 2026 reliability/autonomy review roadmap. Best done after `agent-recall-run-view-memory-search.md` (staged compaction references `run_view`).

Findings:

- Token estimate is `chars / 4` (`crates/bobaclaw-agent/src/context.rs`); the provider already returns `usage.prompt_tokens` every turn but it is discarded. Compaction fires too early (context loss) or too late (API 400s).
- `state.executed` flips on **any** tool, so a single `skills_list` call permanently defeats `ACTION_REQUIRED_NUDGE` (`crates/bobaclaw-agent/src/tool_loop.rs:165-168`) even if `exec` never ran.
- The prompt promises parallel tool calls (`prompt.rs:26`) but the loop executes batches strictly sequentially (`tool_loop.rs:185-219`).
- Compaction's only strategy is LLM-summarize of the middle slice; most history weight is tool bodies that are already duplicated in the run ledger. The summarize call has no cancel token (`crates/bobaclaw-agent/src/compaction.rs:133-145`), delaying interrupts.
- One model serves everything: main turn, summarizer, background review, empty-response retries. Background review results never reach the user (`auto_saved_skill`/`auto_saved_memory` always `None` in `loop_.rs`), and its errors are swallowed.
- Child/subagent loops get no empty-response retries (gated on `TurnMode::Parent`).

## Scope

### In scope

- Record `usage.prompt_tokens` per turn; maintain a per-session chars-per-token ratio and use it in `estimate_tokens` (heuristic stays as cold-start fallback).
- Classify tools into action (`exec`, `schedule*`, `mcp_*`, `subagent`, `spawn`) vs introspection (`skills_list`, `skill_view`, `schedule_list`, `memory_*`, `run_view`); set `executed` only on action tools.
- Execute read-only tool calls from one batch concurrently (`join_all` with cancel propagation); side-effecting tools stay sequential. Alternatively, if deemed too risky, remove the parallelism promise from `prompt.rs` — code and prompt must agree.
- Staged compaction: stage 1 prunes old tool bodies in history to `[exec run_id=… exit=N — full output via run_view]` stubs; stage 2 (existing summarize) only if stage 1 is insufficient.
- Pass the turn cancel token into the compaction summarize call.
- Config knobs `context.summarizer_model` and `agent.review_model` for cheap auxiliary models (default: main model).
- Surface background review outcomes as a progress event / short reply suffix ("saved memory: X"); log its errors at warn level.
- Empty-response retry (1 attempt) for child loops.

### Out of scope

- Streaming token responses from the provider (separate, larger change).
- Verifier pass and auto-continuation (see `agent-verification-and-continuation.md`).
- PicoClaw-style per-request model routing/classification.

## Files likely to change

- `crates/bobaclaw-agent/src/context.rs`, `compaction.rs`, `tool_loop.rs`, `loop_.rs`, `review.rs`, `progress.rs`, `prompt.rs`
- `crates/bobaclaw-agent/src/tools/mod.rs` (action classification)
- `crates/bobaclaw-provider/src/tools_chat.rs` (expose usage)
- `crates/bobaclaw-core/src/agent_config.rs`, `context_config.rs`
- `config.example.yaml`

## Implementation steps

1. Expose provider `usage` to the loop; persist per-session ratio; recalibrate `estimate_tokens`.
2. Add tool action classification; tighten nudge condition; tests for the `skills_list`-defeats-nudge case.
3. Concurrent read-only batch execution with cancel; keep ordering of tool-result messages stable for the API.
4. Stage-1 compaction (tool-body pruning) before summarize; tests for budget recovery without an LLM call.
5. Cancel token into summarize; summarizer/review model config.
6. Surface review results; child empty-response retry.
7. Run validation, including `cargo test -p bobaclaw-agent prompt` (prompt.rs touched).

## Validation

```bash
make ci
cargo test -p bobaclaw-agent -p bobaclaw-provider -p bobaclaw-core
cargo test -p bobaclaw-agent prompt
```

Additional checks:

- Manual: long session — verify stage-1 pruning fires before summarize (tracing).

## Risks

- Parallel tool execution changes message ordering edge cases with picky OpenAI-compatible gateways; keep result append order deterministic.
- Pruned tool bodies remove detail the model might still need; mitigated by `run_view` stubs (dependency on the recall plan).
- Per-session token ratio can be skewed by non-ASCII content; clamp the ratio to a sane range.

## Rollback plan

Revert the branch; all behavior changes are internal to the loop, config fields have serde defaults.

## Completion notes

Fill this after implementation:

- changed files:
- validation run:
- known gaps:
- follow-up work:

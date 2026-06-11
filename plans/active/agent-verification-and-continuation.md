# Agent change plan

## Goal

Add a trigger-based self-verification pass and checkpoint-based continuation for long tasks, so the agent catches "claimed success without evidence" and can resume work past the iteration limit instead of giving up.

## Context

Priority **P3 (exploratory "brain" upgrade)** — part of the June 2026 reliability/autonomy review roadmap. Do after `agent-loop-quality.md` (needs cheap-model routing and accurate token accounting) and `agent-recall-run-view-memory-search.md`.

Findings:

- `prompt.rs` asks the model to "verify results before claiming done", but nothing structural backs it. Failed exec runs followed by a success-sounding final answer go out unchecked.
- On hitting `max_tool_iterations` (60) the loop surrenders: "Reached the tool step limit… Ask to continue or narrow the task" (`crates/bobaclaw-agent/src/tool_loop.rs:305-321`), even though the compaction checkpoint machinery and the `schedule` tool already provide everything needed for resumption.
- The background review loop (every 10 turns) never analyzes **failed** runs from the ledger; Skill Forge only learns from successful runs.

## Scope

### In scope

- Verifier pass, trigger-based only (not every turn): runs when (a) the turn hit the iteration limit, or (b) any exec in the turn failed while the final answer contains success claims. One cheap-model call comparing the final answer against tool outcomes; on mismatch, inject one corrective nudge before delivering.
- Continuation checkpoint: on iteration-limit, write a compaction-style checkpoint and offer the user a one-word "continue" resume; optionally (config-gated, default off) self-schedule a continuation via the existing `schedule` tool with a hard cap on auto-continuations per task.
- Failure-aware review: extend the background review to sample recent failed runs from the ledger and propose skill patches (proposals only; promotion stays manual or approval-gated per `approvals-and-host-danger.md`).
- Config: `agent.verifier_enabled` (default on, cheap model required), `agent.max_auto_continuations` (default 0).

### Out of scope

- A general multi-agent critic architecture.
- Increasing subagent depth beyond 1.
- Automatic skill promotion without operator/approval gate.

## Files likely to change

- `crates/bobaclaw-agent/src/turn.rs`, `tool_loop.rs` (verifier hook, checkpoint-on-limit)
- `crates/bobaclaw-agent/src/verify.rs` (new)
- `crates/bobaclaw-agent/src/review.rs` (failed-run sampling)
- `crates/bobaclaw-agent/src/compaction.rs` (checkpoint reuse)
- `crates/bobaclaw-core/src/agent_config.rs`
- `crates/bobaclaw-state/src/ledger.rs` (failed-run query)
- `config.example.yaml`
- `harness/` docs if tool semantics change

## Implementation steps

1. `verify.rs`: build the verifier prompt from tool persist entries + final text; trigger conditions; single corrective nudge path.
2. Checkpoint-on-limit: reuse compaction summary as a resumable checkpoint; "continue" handling in the next turn.
3. Config-gated auto-continuation via `schedule` with counter and cap.
4. Failed-run sampling in background review; proposal formatting.
5. Tests: verifier catches a fabricated success; continuation resumes with checkpoint context; auto-continuation respects the cap.
6. Run validation.

## Validation

```bash
make ci
cargo test -p bobaclaw-agent
```

Additional checks:

- Eval scenario: a task that requires > 60 steps completes across two continuations with `max_auto_continuations: 1`.

## Risks

- Verifier adds latency and cost to failure paths; trigger-based scoping and cheap-model routing keep it bounded.
- Auto-continuation can loop on an impossible task; hard cap + checkpoint diffing ("no progress since last checkpoint → stop") required.
- Corrective nudge may cause the model to over-hedge; limit to one retry.

## Rollback plan

Revert the branch; both features are config-gated and additive to the loop.

## Completion notes

Fill this after implementation:

- changed files:
- validation run:
- known gaps:
- follow-up work:

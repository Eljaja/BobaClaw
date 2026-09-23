# Agent change plan: compaction boundary fix

## Goal

Session compaction must not drop the most recent messages (including the current user request).

## Context

`summarize_slice` kept the last `keep_recent_messages` rows unsummarized, but the `compaction`
row was appended after them and `effective_history` returned rows from the last compaction row
onward. The kept tail was excluded from context and never summarized later (the next slice
started after the compaction row), so it was lost. `loop_.rs` appends the user message before
compaction, so on a compaction turn the model did not see the current request.

## Scope

### In scope

- Record the id of the last summarized message on each compaction row.
- Effective history = latest summary + all non-compaction messages after its boundary.
- Next compaction summarizes from the previous boundary (plus previous summary).
- Keep `maybe_ensure_context_budget` history/in-turn split correct across repeated rebuilds.

### Out of scope

- Tool-call/result pairing in persisted history; loop_.rs ordering; gateway/executor.

## Files likely to change

- `migrations/20260610100000_compaction_boundary.sql`
- `crates/bobaclaw-state/src/session.rs`, `crates/bobaclaw-state/src/lib.rs`
- `crates/bobaclaw-agent/src/compaction.rs`, `turn.rs`, `tool_loop.rs`

## Implementation steps

1. Add nullable `messages.covers_through_id` column.
2. Add `StoredMessage`, `list_stored_messages`, `append_compaction` to `SessionStore`.
3. Rework compaction slicing/effective history around the boundary; legacy rows (NULL
   boundary) keep the old "from the compaction row onward" behaviour.
4. Make `history_boundary` in the tool loop `&mut` so it tracks the rebuilt layout.

## Validation

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace --no-fail-fast
make ci
```

## Risks

- Tail split is by row count and may start on an assistant row (unchanged from before).

## Rollback plan

Revert the commit. The added column is nullable and ignored by older code.

## Completion notes

- changed files: listed above.
- validation run: fmt clean; clippy no new warnings in touched files; workspace tests pass
  except 2 pre-existing `bobaclaw-executor` docker_mount failures (macOS path issue, on main);
  `make ci` harness checks OK, fails only on those same 2 tests.
- known gaps: legacy compaction rows without a boundary still hide their pre-row tail.
- follow-up work: none required.

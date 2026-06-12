# Agent change plan: Memory system v2 (tiered memory per ADR 005)

## Goal

Turn BobaClaw's write-only memory into a closed loop — capture → consolidate → recall → forget — per [ADR 005](../../docs/adr/005-memory-architecture.md), reusing existing FTS5, Run Ledger, and workspace markdown infrastructure.

## Context

- Current state and gaps are inventoried in ADR 005 and `docs/as-built.md`: injection caps make memory silently lossy, `messages_fts` is never queried, background review writes are invisible, exec truncation has no pointer to the Run Ledger.
- This plan **coordinates** and does not duplicate: recall tools are specced in `agent-recall-run-view-memory-search.md`, operator search in `f6-history-search.md`, the write gate in `f13-memory-integrity.md`, staged compaction in `agent-loop-quality.md`. Items below are the *new* work ADR 005 adds.
- Memory poisoning is the primary attack vector (`plans/backlog.md`); every new write path here must route through the F13 gate once it lands, and must be safe (append-only, operator-visible) before then.

## Scope

### In scope

1. **Surface background review writes.** Wire `PostTurnReviewOutcome` into `AgentResponse.auto_saved_memory` and render it in CLI/Telegram ("saved to memory: <summary>"). No silent self-modification.
2. **Daily note auto-routing.** `memory_manage(append)` without an explicit `file` writes dated/ephemeral content to `memory/YYYY-MM-DD.md` (auto-created); durable facts still target `MEMORY.md`. Matches the behaviour already documented in `workspace-examples/home/BOBACLAW.md` but not implemented.
3. **Compaction durable-facts hook.** Add a mandatory "Durable facts" section to the compaction summary template (`prompt.rs` summarizer template); after compaction, offer those lines to memory via the same gated write path used by review.
4. **Consolidation job.** Scheduled task (off by default, config-keyed) that merges daily notes older than N days into topic files / `MEMORY.md`, deduplicates, and demotes stale content to `memory/archive/` (excluded from prompt injection by `load_memory_dir`, still readable/searchable). All rewrites recorded with diffs; routed through F13 gate when available, operator-notified always.
5. **Memory-file FTS.** Index workspace memory files so `memory_search` (from the recall plan) covers curated memory, not only session transcripts.
6. **Subagent read-only memory.** `memory_read` / `memory_search` available to child subagents behind a config flag, default off. Writes remain parent-only.
7. **Config keys** for: consolidation enable/interval/age threshold, review interval (currently a constant), subagent memory read flag, injection budgets.
8. **Docs:** ADR 005 status → accepted on merge of first increment; `docs/as-built.md` sections per shipped increment; `harness/tools/memory.md` updates.

### Out of scope

- `run_view`, `memory_search`, `memory_read` tool implementations — owned by `agent-recall-run-view-memory-search.md` (land that first).
- FTS CLI/gateway surface — F6.
- Versioned write gate, drift detection, rollback — F13.
- Vector/embedding memory, user-model service — rejected for v1 in ADR 005.

## Files likely to change

- `crates/bobaclaw-agent/src/loop_.rs`, `review.rs` — surface `auto_saved_memory`
- `crates/bobaclaw-agent/src/tools/memory.rs` — daily-note routing
- `crates/bobaclaw-agent/src/prompt.rs`, `compaction.rs` — durable-facts section + hook
- `crates/bobaclaw-scheduler/` — consolidation job
- `crates/bobaclaw-state/` — memory-file FTS index
- `crates/bobaclaw-agent/src/tools/specs.rs` — subagent read-only specs
- `crates/bobaclaw-core/src/context_config.rs`, `config.example.yaml` — new keys
- `harness/tools/memory.md`, `docs/as-built.md`, `docs/adr/005-memory-architecture.md`
- `crates/bobaclaw-channel-telegram/`, `crates/bobaclaw/` (CLI) — render auto-saved notices
- `evals/` — recall smoke scenario (fact saved in one session recalled in another)

## Implementation steps

Each step is an independently shippable PR, in dependency order:

1. Land `agent-recall-run-view-memory-search.md` (prerequisite — recall tools).
2. Surface background review writes (item 1) + config key for review interval.
3. Daily-note auto-routing in `memory_manage` (item 2).
4. Compaction durable-facts hook (item 3) — requires plan approval for `prompt.rs` changes per `plans/AGENTS.md`; coordinate with staged compaction in `agent-loop-quality.md`.
5. Memory-file FTS + extend `memory_search` scope (item 5).
6. Consolidation job, off by default (item 4); enable-by-default only after F13 gate exists.
7. Subagent read-only memory flag (item 6).
8. Flip ADR 005 to accepted; update `docs/as-built.md`.

## Validation

```bash
make ci
cargo test -p bobaclaw-agent -p bobaclaw-state -p bobaclaw-scheduler
make eval-smoke
```

Additional checks:

- Eval: save a fact in session A, compact, recall it in session B via search rather than guessing.
- Manual: verify auto-saved memory notice appears in CLI and Telegram.
- Verify `memory/archive/` is excluded from prompt injection but returned by `memory_search`.

## Risks

- Consolidation rewrites memory — the highest-risk new capability. Mitigation: off by default, full diffs recorded, operator notification, F13 gate before default-on.
- More injected hints / tool specs can hurt prompt-cache hit rate — keep additions short, fixed English strings.
- Daily-note auto-routing heuristic may misfile durable facts — keep explicit `file` argument authoritative; review job catches misfiles.

## Rollback plan

Each increment is additive and individually revertable. Consolidation and subagent read access are config-gated (default off) — disable via config without code rollback. No destructive migrations: memory-file FTS index is a new table/derivable index.

## Completion notes

Fill this after implementation:

- changed files:
- validation run:
- known gaps:
- follow-up work:

# ADR 005: Tiered memory architecture

**Status:** proposed  
**Date:** 2026-06-12

## Context

BobaClaw's memory today is split across mechanisms that do not form a loop:

- **Session history** lives in SQLite `messages` (`crates/bobaclaw-state`), compacted by an LLM summary row (`crates/bobaclaw-agent/src/compaction.rs`). After compaction, everything before the last `compaction` row is invisible to the model.
- **Long-term memory** is workspace markdown (`MEMORY.md`, `memory/*`) injected into the system prompt with hard caps (20k chars per file, 8k total for `memory/`, 2k per `memory/` file — `prompt.rs`). The only write path is `memory_manage(append)`.
- **`messages_fts`** (FTS5 + triggers) has existed since the initial migration with zero queries in code.
- **Run Ledger** (`runs/<id>/`, `runs`/`run_events` tables) stores full exec output, but truncated tool bodies carry no pointer back to it.
- **Background memory review** (`review.rs`, every 10th user message) writes memory the operator never sees; `auto_saved_memory` is never populated.

Net effect: memory is **write-only past the injection caps**. Once `MEMORY.md` or `memory/` outgrows the budget, the agent silently forgets its own notes; once a session compacts, prior detail is unrecoverable; exec output past 24k chars forces re-running commands.

Related accepted direction: ADR 002 (state.db + Run Ledger), `plans/backlog.md` F6 (history search), F13 (memory integrity — memory poisoning is the primary post-OpenClaw attack vector), and active plans `agent-recall-run-view-memory-search.md` and `agent-loop-quality.md` (staged compaction).

## Decision

Adopt a **four-tier memory model** with an explicit lifecycle (capture → consolidate → recall → forget), built on infrastructure that already exists (SQLite FTS5, Run Ledger, workspace markdown). No new storage engine, no embeddings in v1.

### Tiers

| Tier | Contents | Store | Recall path |
|------|----------|-------|-------------|
| **T0 Working** | Current turn + effective history since last compaction | `messages` (in-context) | Always in context |
| **T1 Episodic** | Full session transcripts across sessions; exec runs | `messages` + `messages_fts`; Run Ledger | `memory_search` / `history_search`, `run_view` |
| **T2 Curated** | Distilled durable facts, preferences, project state | `MEMORY.md` (core) + `memory/*.md` (topics, daily notes) | Prompt injection (budgeted) + `memory_read` / `memory_search` |
| **T3 Behavioural** | Identity and standing instructions | `SOUL.md`, `BOBACLAW.md`, `USER.md` | Always injected; writes gated by F13 |

Design rule: **injection is a cache, not the store.** Everything injected must also be reachable by a read/search tool, so exceeding an injection budget degrades to "one tool call away" instead of "gone".

### Lifecycle

**Capture.**

- `memory_manage(append)` stays the primary in-turn write path (facts → `MEMORY.md` or `memory/<topic>.md`; transient observations → daily note `memory/YYYY-MM-DD.md`, auto-created by the tool when `file` is omitted and content is dated/ephemeral).
- Background post-turn review (`review.rs`) keeps running, but its writes are surfaced: `auto_saved_memory` is wired through `AgentResponse` so channels/CLI can show "saved to memory: …". Invisible self-modification is a trust and security defect, not a feature.
- Compaction rows are capture too: the structured summary template gains a mandatory **"Durable facts"** section; the post-compaction hook offers those lines to `memory_manage` (subject to F13 gating) instead of letting them die inside a summary that the next compaction will itself summarize away.

**Consolidate.**

- A scheduled low-priority job (reusing `bobaclaw-scheduler`) periodically distills: daily notes older than N days → merged/deduplicated into topic files or `MEMORY.md`; superseded facts rewritten rather than appended forever. This is the only path allowed to *rewrite* memory files, and it runs through the F13 versioned write gate (hash chain, diff recorded, operator-visible).
- `MEMORY.md` has a soft size target (≤ injection cap). Consolidation keeps the core small and pushes detail into topic files, which are searchable rather than injected.

**Recall.**

- `memory_read(path, range?)` — read any workspace memory file beyond injection caps.
- `memory_search(query, scope?)` — one tool, two backends: FTS5 over `messages_fts` (episodic) and a lightweight FTS index over memory files (curated). Results carry provenance (session/date/file) and, once F5 lands, taint markers.
- `run_view(run_id, …)` — full exec output recall; truncated exec bodies embed the pointer ("full output: run_view <id>").
- Subagents get **read-only** memory access (`memory_read`, `memory_search`) behind a config flag, default off. Write access stays parent-only.

**Forget.**

- Forgetting is *demotion*, not deletion: consolidation moves stale content from injected files into `memory/archive/` (excluded from injection, still searchable). Hard deletion is operator-only (F13 rollback/restore CLI covers the audit trail).

### Security boundary (delegates to F13/F5)

- All memory writes flow through one gate: content-addressed version chain, `source ∈ operator|agent|review|consolidation`, taint propagation. Untrusted-tainted turns cannot write T3 files; instruction-like content in T2 writes routes to F1 approval.
- Search results are data, not instructions: prompt text already states compaction rows are reference-only; the same framing applies to `memory_search` snippets.

### Explicitly rejected for v1

- **Vector/embedding store** — premature until FTS recall is actually used; revisit (v2) only if FTS demonstrably misses recall cases. Keeps the runtime dependency-free and local-first. The post-FTS path (hybrid sqlite-vec search, reconciling consolidation, bi-temporal facts) is surveyed in [docs/memory-beyond-fts.md](../memory-beyond-fts.md).
- **Honcho-style user modeling service** — `USER.md` + curated memory covers the need at this scale.
- **Automatic deletion / TTL** — silent forgetting is the failure mode we are fixing; archive instead.
- **Per-message embeddings of tool output** — Run Ledger + `run_view` already give exact recall.

## Consequences

- Memory becomes a closed loop: anything the agent wrote or saw is reachable via injection, search, or ledger — bounded context cost, unbounded recall.
- Three active plans implement this ADR incrementally and stay independently shippable: `agent-recall-run-view-memory-search.md` (recall tools), `f6-history-search.md` (operator-facing search), `f13-memory-integrity.md` (write gate). New work items (consolidation job, review surfacing, daily-note auto-routing, subagent read access) are tracked in `plans/active/memory-system-v2.md`.
- Prompt cache stays stable: new prompt text is limited to short, fixed English hints about the recall tools; injection caps and file layout are unchanged.
- One new moving part (consolidation job) that can rewrite memory — mitigated by running it through the F13 gate with full diffs and operator notification.

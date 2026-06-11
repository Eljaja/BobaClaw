# Agent change plan

## Goal

Give the agent recall: a `run_view` tool over the existing run ledger, `memory_search` over the existing FTS5 schema, and `memory_read` — turning write-only memory and audit-only ledger into queryable working memory.

## Context

Priority **P1 (autonomy / "brain")** — part of the June 2026 reliability/autonomy review roadmap. Highest leverage-to-cost item: most infrastructure already exists.

Findings:

- Exec output sent to the model is truncated to 24,000 chars head/tail, but full stdout/stderr already live in `runs/<id>/` and the ledger. The model re-runs commands to recover lost output. A `run_view(run_id, ...)` tool closes this loop for free.
- `messages_fts` (FTS5 + triggers) has existed since the initial migration with **zero queries in code**. Session recall beyond the context window is impossible today.
- `memory_manage` supports only `append`; reads happen via prompt injection capped at 8,000 chars per `memory/` dir and 2,000 per file (`crates/bobaclaw-agent/src/prompt.rs`). Once memory outgrows the caps, the agent forgets what it wrote.
- Skill matching is substring-on-name/tags and the system prompt lists only skill names; descriptions are invisible to the model until it calls `skill_view`.

## Scope

### In scope

- New tool `run_view`: fetch a past run's full stdout/stderr by `run_id` with optional line range or grep filter; output truncated to the standard tool body cap with head/tail.
- New tool `memory_search`: FTS5 query over `messages_fts` (and memory files), returning snippets with session/date context.
- New tool `memory_read`: read `MEMORY.md` / `memory/<file>` beyond injection caps.
- Reference `run_id` in truncated exec bodies ("full output: run_view <id>").
- Inject skill descriptions (not just names) into the system prompt; widen matching to description words.
- Harness contract docs for the new tools (`harness/tools/`).
- Prompt hints (short, stable English additions to the memory/exec sections of `prompt.rs`).

### Out of scope

- Vector/embedding memory — premature until FTS recall is in use.
- Memory editing/deletion semantics (`memory_manage` stays append-only here).
- A user-facing `bobaclaw search` CLI (natural follow-up, separate change).

## Files likely to change

- `crates/bobaclaw-agent/src/tools/` (new `run_view.rs`, `memory.rs` extensions, `specs.rs`, `router.rs`)
- `crates/bobaclaw-agent/src/tools/exec.rs` (run_id hint in truncation)
- `crates/bobaclaw-agent/src/prompt.rs` (skill descriptions, memory/exec hints)
- `crates/bobaclaw-state/src/` (FTS query API, ledger read API)
- `crates/bobaclaw-skills/src/registry.rs` (description matching)
- `harness/tools/run_view.md`, `harness/tools/memory.md` (new/updated)
- `evals/` (smoke scenario: recall a fact saved in an earlier session)

## Implementation steps

1. State APIs: `search_messages(query, limit)` over FTS5; `get_run_output(run_id)` from ledger/artifacts.
2. Implement `run_view` tool + spec + router entry; add run_id hint to exec truncation message.
3. Implement `memory_search` and `memory_read` tools; restrict paths to workspace memory files.
4. Skill descriptions in prompt + matching widening; keep prompt-cache key stable (descriptions are part of registry load).
5. Harness docs + eval scenario.
6. Run validation.

## Validation

```bash
make ci
cargo test -p bobaclaw-agent -p bobaclaw-state -p bobaclaw-skills
make eval-smoke
```

Additional checks:

- Manual: ask the agent about a fact stored two sessions ago; verify it uses `memory_search` instead of guessing.

## Risks

- FTS results can leak other sessions' content into the current group; scope queries by agent group/session policy.
- More tools = more prompt surface; keep specs terse to protect prompt-cache hit rate.

## Rollback plan

Revert the branch; tools are additive, no schema migration required (FTS already exists).

## Completion notes

Fill this after implementation:

- changed files:
- validation run:
- known gaps:
- follow-up work:

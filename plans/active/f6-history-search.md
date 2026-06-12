# Agent change plan: F6 — History search (FTS5)

## Goal

Expose existing `messages_fts` via `SessionStore::search`, a parent-agent tool, CLI, and gateway API — with taint markers in snippets.

## Context

- FTS5 schema and triggers exist; no search API (`docs/as-built.md`).
- Near-free capability once F5 taint markers exist in results.
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

1. `SessionStore::search(query, scope?, limit)` over `messages_fts` (BM25 ranking, `snippet()`).

2. **Tool:** `history_search { query, limit?, scope? }` — parent agent only (not subagents by default).

3. **Results:** ts, session, role, snippet (+ taint marker when F5 landed).

4. **CLI:** `bobaclaw history search "<query>" [--group]`.

5. **Gateway:** `GET /api/history/search?q=`.

6. Deleted sessions excluded from results.

### Out of scope

- Semantic/vector search.
- Cross-tenant search.

## Files likely to change

- `crates/bobaclaw-state/` — search implementation
- `crates/bobaclaw-agent/src/tools/` — `history_search`
- `crates/bobaclaw-gateway/`
- `crates/bobaclaw/` — CLI
- `harness/tools/history-search.md`
- `docs/as-built.md`

## Implementation steps

1. Implement FTS query with BM25 + snippet.
2. Scope/group filtering.
3. Register tool (parent only; config to allow subagents later).
4. Gateway route.
5. CLI command.
6. Taint marker in snippet when message untrusted.
7. Harness: insert messages, search via tool and CLI, verify ranking; deleted session absent.

## Validation

```bash
make ci
```

## Risks

- FTS query injection — sanitize/limit query length.
- Large result sets — enforce `limit` default.

## Rollback plan

- Remove tool and routes; FTS schema unchanged.

## Dependencies

- Soft: F5 for taint in snippets (can ship without, add marker in follow-up).

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work:

# Tool: memory_manage / memory_search / memory_read

Workspace long-term memory and recall. Implementation: `crates/bobaclaw-agent/src/tools/memory.rs`.

## Purpose

- **memory_manage** — append durable information to `MEMORY.md` or `memory/`
- **memory_search** — FTS search over past session messages + workspace memory files
- **memory_read** — read memory files beyond prompt injection caps

Not for repeatable multi-step tool workflows — use skills for those.

## Input

### memory_manage

```json
{
  "action": "append",
  "path": "MEMORY.md | memory/<file>",
  "content": "string"
}
```

### memory_search

```json
{ "query": "search terms", "limit": 10 }
```

### memory_read

```json
{ "path": "MEMORY.md | memory/<file>", "offset": 1, "limit": 100 }
```

Allowed paths for manage/read:

- `MEMORY.md` at workspace group root
- `memory/<basename>.md` or `.txt` (single segment only)

## Side effects

- **memory_manage**: appends under `~/.bobaclaw/workspace/<group>/`
- **memory_search** / **memory_read**: read-only

## Answer contract

When using **memory_search** or **memory_read** results in the user-facing answer, cite the memory file path in `## Sources`.

## Approval requirements

- Low risk: workspace-scoped; manage is append-only in v1.

## Failure modes

- Invalid path (traversal, wrong extension) — fix `path`
- Size limit exceeded on append — shorten content or split across files

## Telemetry

File path appended (manage); no Run Ledger capsule.

## Tests

`cargo test -p bobaclaw-agent memory`

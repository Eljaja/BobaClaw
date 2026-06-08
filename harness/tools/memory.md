# Tool: memory_manage

Workspace long-term memory (facts, preferences, user context). Implementation: `crates/bobaclaw-agent/src/tools/memory.rs`.

## Purpose

Append durable information to `MEMORY.md` or files under `memory/`. Not for repeatable multi-step tool workflows — use skills for those.

## Input

```json
{
  "action": "append",
  "path": "MEMORY.md | memory/<file>",
  "content": "string"
}
```

Required: `action`, `path`, `content`.

Allowed paths:

- `MEMORY.md` at workspace group root
- `memory/<basename>.md` or `memory/<basename>.txt` (single segment only)

## Side effects

- Appends text under `~/.bobaclaw/workspace/<group>/`.
- Creates `memory/` directory when needed.
- Per-append cap: 4 KB. Total file cap: 64 KB.

## Approval requirements

- Low risk: workspace-scoped append only.
- No delete or replace in v1.

## Failure modes

- Invalid path (traversal, wrong extension) — fix `path` and retry.
- Size limit exceeded — shorten content or split across files.

## Telemetry

File path appended; no Run Ledger capsule.

## Tests

`cargo test -p bobaclaw-agent memory`

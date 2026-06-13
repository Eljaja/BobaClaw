# Tool: file_read / file_write / file_edit

Workspace-scoped file tools. Implementation: `crates/bobaclaw-agent/src/tools/files.rs`.

## Purpose

Read, write, and edit files inside the agent group workspace without shell escaping. Prefer over `exec` + cat/sed/heredoc.

## Input

### file_read

```json
{ "path": "relative/path", "offset": 1, "limit": 100 }
```

### file_write

```json
{ "path": "relative/path", "contents": "string" }
```

### file_edit

```json
{ "path": "relative/path", "old_string": "…", "new_string": "…", "replace_all": false }
```

Paths must be relative to the workspace root. No `..`, no absolute paths.

## Side effects

- `file_read`: none
- `file_write` / `file_edit`: creates or mutates workspace files

## Approval requirements

- Low risk: workspace-scoped only; path traversal and symlink escape blocked.

## Failure modes

- Path escapes workspace — fix `path`
- `file_edit`: old_string not found or ambiguous — adjust strings or set `replace_all`

## Telemetry

No Run Ledger capsule (unlike `exec`).

## Tests

`cargo test -p bobaclaw-agent files`

# Tool: run_view

Fetch full exec stdout/stderr by run_id. Implementation: `crates/bobaclaw-agent/src/tools/run_view.rs`.

## Purpose

Recover truncated `exec` output from run capsules when the tool body exceeded the 24k char cap.

## Input

```json
{
  "run_id": "run_…",
  "stream": "stdout | stderr | both",
  "grep": "optional substring filter"
}
```

## Side effects

- Read-only access to run artifacts on disk and ledger metadata

## Scope

- Run must belong to a session in the current agent group

## Tests

`cargo test -p bobaclaw-agent run_view`

# Tool: subagent (and spawn)

Isolated delegation loops for focused subtasks. Implementation: `crates/bobaclaw-agent/src/subagent/`, `crates/bobaclaw-agent/src/tools/subagent.rs`, `crates/bobaclaw-agent/src/tools/spawn.rs`.

## Purpose

Run a **fresh-context** agent loop for multi-step or context-heavy work. Parent integrates the structured summary into the user-facing reply.

Use **`subagent`** when the parent should **wait** for the result in the same turn.

Use **`spawn`** for fire-and-forget background work; completion is appended to the parent session.

## When to delegate (parent agent)

**Call when:**

- Multi-step research, many files, or isolated implementation slice.
- Parent context is large/noisy and the deliverable is a summary or artifact.
- Parallel read-heavy subtasks (up to `subagents.max_concurrent`).

**Do not call when:**

- One `exec` or one MCP call suffices.
- Direct factual question from memory or a single tool call.
- Task needs memory/skills/schedule writes (child cannot).
- Nested delegation (`max_depth: 1` — child cannot call `subagent` / `spawn`).

## Input (`subagent`)

```json
{
  "task": "string (required, self-contained goal + scope + expected output)",
  "label": "string (optional, logs)",
  "context": "string (optional, short parent snippet)",
  "preset": "string (optional, config subagents.presets.<id>)",
  "backend": "string (optional: native | claude-code | codex | cursor)"
}
```

## Child tool allowlist (native backend)

| Tool | Child | Notes |
|------|:-----:|-------|
| `exec` | yes | sandbox workspace |
| `mcp_*` | yes | configured MCP |
| `skills_list`, `skill_view` | yes | read procedures |
| `skill_manage`, `memory_manage` | no | parent owns persistence |
| `schedule*` | no | parent session delivery |
| `subagent`, `spawn` | no | depth limit |

Presets may widen allowlist via `tools_allowlist`.

## Side effects

- Native: ephemeral in-memory child messages; optional `persist_child_sessions` writes `sessions.parent_session_id`.
- Run Ledger entry per subagent run (`subagent_*` id).
- External backends: sandboxed CLI subprocess with dedicated capsule (longer timeout).
- `spawn`: background task; appends `[Subagent … completed]` assistant message on success.

## Approval requirements

- **Native:** medium risk (extra LLM cost; bounded by `max_tool_iterations` and `child_timeout_seconds`).
- **External CLI backends (`claude-code`, `codex`, `cursor`):** high risk — disabled by default; separate API keys; see `harness/policy.md`.

## Failure modes

- Empty `task` → validation error.
- Unknown `preset` → validation error.
- `max_depth` exceeded → error (nested subagent).
- Semaphore timeout / `max_concurrent` → structured error.
- Child timeout → error after `child_timeout_seconds`.
- Result truncated to `result_max_chars` for parent context.

## Telemetry

- Progress: `SubagentStart`, `SubagentEnd` (label, exit code, preview).
- Run Ledger: subagent run id, parent session id, backend name, exit code.

## Tests

```bash
cargo test -p bobaclaw-agent subagent prompt specs tool_loop
cargo test -p bobaclaw-core subagent
```

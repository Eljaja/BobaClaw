# Tool: subagent (and spawn)

Isolated delegation loops for focused subtasks. Implementation: `crates/bobaclaw-agent/src/subagent/`, `crates/bobaclaw-agent/src/tools/subagent.rs`, `crates/bobaclaw-agent/src/tools/spawn.rs`.

## Purpose

Run a **fresh-context** agent loop for multi-step or context-heavy work. Parent integrates the structured summary into the user-facing reply.

Use **`subagent`** when the parent should **wait** for the result in the same turn.

Use **`spawn`** for fire-and-forget background work. Jobs are persisted in `spawn_jobs` (SQLite); completion is appended to the parent session and may notify/wake the parent per `subagents.spawn` config.

Use **`spawn_status`** to query a job in the **current session** by `task_id` and/or `label`.

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

## Input (`spawn`)

```json
{
  "task": "string (required)",
  "label": "string (optional)",
  "context": "string (optional)",
  "preset": "string (optional)",
  "backend": "string (optional)",
  "wake": "boolean (optional, default from subagents.spawn.wake_parent_on_complete)"
}
```

Returns `task_id` (`spawn_<uuid>`). Status and result preview are queryable via `spawn_status` or operator surfaces (`/subagents`, gateway API).

## Input (`spawn_status`)

```json
{
  "task_id": "string (optional, spawn_<uuid>)",
  "label": "string (optional, latest match in session)"
}
```

At least one of `task_id` or `label` is required. Scope is the **current session only** — jobs from other sessions return not found.

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
- `spawn`: background task row in `spawn_jobs`; on success appends `[Subagent … completed]` to session, optional channel notify, optional parent wake (`IngressKind::SpawnWake`).
- `spawn_status`: read-only query of `spawn_jobs` for the current session.

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

## Spawn feedback config (`subagents.spawn`)

| Key | Default | Purpose |
|-----|---------|---------|
| `notify_on_complete` | `true` | Short push via `ChannelDelivery` (`cli` outbox, `telegram` sendMessage) |
| `wake_parent_on_complete` | `true` | Synthetic user turn after success |
| `wake_on_failure` | `false` | Wake on non-zero exit |
| `wake_max_per_hour_per_session` | `10` | Rate limit wakes |
| `result_persist_chars` | `12000` | Truncate `result_body` in DB |
| `job_retention_days` | `30` | Retention (cleanup deferred) |

Wake is skipped when `session:{id}` is busy or rate limit exceeded. Job still finalizes and session history is updated.

## Operator surfaces

- CLI / Telegram `/subagents` — list jobs for current session (`SpawnJobStore::list_by_session`).
- Gateway: `GET /api/spawn/jobs?session_id=`, `GET /api/spawn/jobs/:id` (operator-local, same access as `/api/agent`).

## Tests

```bash
cargo test -p bobaclaw-state spawn
cargo test -p bobaclaw-agent spawn subagent prompt specs tool_loop
cargo test -p bobaclaw-core subagent
cargo test -p bobaclaw-gateway
```

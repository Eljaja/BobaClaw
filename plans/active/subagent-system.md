# Agent change plan: Subagent system (native + external backends)

## Goal

Add a unified delegation layer to BobaClaw so the parent agent can spawn focused child runs with isolated context, starting with in-process native subagents and extending to optional external backends (Cursor SDK, Claude Code CLI, Codex CLI).

## Context

- BobaClaw has no subagent/delegate tools today (`docs/features.md` P2 gap vs Hermes, PicoClaw, NullClaw).
- The runtime already runs **secondary LLM loops** in `review.rs` (memory/skill background review) with restricted tools — this is the closest precedent.
- Main turn logic lives in `turn.rs` (`run_agent_turn`); tools are registered inline; there is no shared `ToolLoopRunner`.
- Parallelism today is **scope-level** only (`AgentDispatcher` + `gateway.max_parallel_turns`); subagents need their own concurrency and depth limits.
- External CLI agents (Claude Code, Codex, Cursor) introduce **double billing, double sandboxing, and credential isolation** concerns — they must be opt-in backends behind harness contracts and approval gates, not raw `exec` wrappers.
- Prompt policy (`AGENTS.md`): durable delegation *usage* rules may go in `prompt.rs`; backend commands, config keys, and executor details belong in `config.yaml`, harness docs, and workspace `BOBACLAW.md`.

Reference patterns (not copied verbatim):

- PicoClaw: `subagent` (sync) + `spawn` (async), `SubagentManager`, max depth 1
- OpenClaw-style: child toolset excludes `spawn_subagent`
- BobaClaw `review.rs`: ephemeral messages, no parent session pollution

## Scope

### In scope

**Phase A — Native subagent (first PR, required for MVP)**

- Extract a reusable tool loop runner from `turn.rs` (shared by main turn, subagent, and optionally `review.rs` later).
- Add `SubagentManager` + synchronous `subagent` tool.
- Child run: ephemeral in-memory conversation (no writes to parent session DB).
- Child toolset = parent tools **minus** `subagent` / `spawn` (enforce `max_depth = 1`).
- Config defaults: `subagents.max_depth`, `subagents.max_concurrent`, `subagents.max_tool_iterations` (lower than parent).
- Cancellation via parent `CancellationToken`; progress events (`SubagentStart` / `SubagentEnd`).
- Run Ledger / capsule metadata for subagent runs (link to parent `run_id` when available).
- Harness contract `harness/tools/subagent.md`.
- Unit tests + prompt tests for delegation hints.
- Short delegation section in `prompt.rs` (when to delegate, not how backends work).

**Phase B — Presets and targeting**

- Optional `agent_id` / `preset` parameter on `subagent` tool.
- Configured presets in `config.yaml` (model override, system prompt snippet, tool allowlist).
- Allowlist checker (reject unknown preset IDs).

**Phase C — External backends (opt-in, separate PRs)**

- Pluggable `SubagentBackend` trait; `subagent` tool gains `backend` parameter (`native` default).
- **Cursor**: `@cursor/sdk` / `cursor-sdk` via `Agent.prompt` (local `cwd` = workspace); `api_key_env`.
- **Claude Code**: sandboxed CLI invocation (long timeout, stdout capture, run capsule).
- **Codex**: sandboxed CLI invocation (same pattern).
- All external backends `enabled: false` by default; high-risk approval when profile requires host/network.

**Phase D — Async spawn + operator UX (optional follow-up)**

- `spawn` tool (fire-and-forget) with completion delivery (session append or channel notify).
- `bobaclaw subagents list` debug command or gateway admin endpoint.

### Out of scope

- Recursive subagents (`max_depth > 1`).
- Subagents as separate `agent_group` workspaces (groups remain isolation boundaries, not delegation targets).
- MCP wrapper for every external CLI on day one.
- Autonomous subagent spawning without parent tool call.
- Streaming subagent output to Telegram in v1.
- ClawHub / marketplace agent definitions.

## Files likely to change

**Phase A**

- `plans/active/subagent-system.md`
- `crates/bobaclaw-agent/src/tool_loop.rs` (new — extracted runner)
- `crates/bobaclaw-agent/src/subagent/mod.rs` (new — manager + types)
- `crates/bobaclaw-agent/src/tools/subagent.rs` (new)
- `crates/bobaclaw-agent/src/tools/mod.rs`
- `crates/bobaclaw-agent/src/turn.rs`
- `crates/bobaclaw-agent/src/lib.rs`
- `crates/bobaclaw-agent/src/progress.rs` (events)
- `crates/bobaclaw-agent/src/prompt.rs`
- `crates/bobaclaw-core/src/agent_config.rs` or new `subagent_config.rs`
- `crates/bobaclaw-core/src/config.rs`
- `config.example.yaml`
- `harness/tools/subagent.md`
- `workspace-examples/home/BOBACLAW.md`
- `docs/features.md` (matrix update when Phase A ships)

**Phase C (additional)**

- `crates/bobaclaw-agent/src/subagent/backends/` (native, cursor, claude_code, codex)
- `harness/policy.md` (external backend risk class)

## Implementation steps

### Phase A — Native subagent

1. **Config schema** — add `subagents` section with defaults:
   - `max_depth: 1`
   - `max_concurrent: 2`
   - `max_tool_iterations: 20`
   - `enabled: true` (native only until backends exist)

2. **Extract `run_tool_loop`** from `turn.rs` into `tool_loop.rs`:
   - Inputs: messages, tools, client, cancel, progress, iteration cap, tool dispatch closure.
   - Outputs: final text, tool call count, executed flag, tool persist entries, interrupted flag.
   - Keep `run_agent_turn` as orchestrator (session load, compaction, system prompt, post-turn snapshot).

3. **SubagentManager** (`subagent/mod.rs`):
   - Semaphore for `max_concurrent`.
   - `run_sync(task, label, preset, parent_ctx) -> SubagentResult`.
   - Build child system prompt (minimal subagent identity + task).
   - Build child tool list via `tools_for_subagent(parent_tools)` — filter out delegate tools.
   - Run `run_tool_loop` with ephemeral `Vec<ConversationMessage>`; do not touch parent session store.

4. **`subagent` tool** (`tools/subagent.rs`):
   - Parameters: `task` (required), `label` (optional), `preset` (optional, Phase B stub OK).
   - Handler calls `SubagentManager::run_sync`; returns truncated result to LLM + capsule reference.
   - Wire into `turn.rs` tool dispatch (alongside exec, memory, mcp, skills).

5. **Telemetry** — extend Run Ledger with `parent_run_id`, `subagent_id`, `label`; emit progress events.

6. **Prompt** — add concise delegation guidance:
   - Use for research, large file analysis, focused implementation with fresh context.
   - Do not delegate trivial one-shot questions.
   - Subagents cannot spawn subagents.

7. **Harness** — `harness/tools/subagent.md` per template (side effects, approval, timeout, failure modes).

8. **Tests**:
   - `subagent` tool rejects empty task.
   - Child tool list excludes `subagent`.
   - Concurrency semaphore blocks when at limit (unit test with low limit).
   - Prompt contains delegation section.
   - Refactor safety: existing `turn` tests still pass.

9. Run `make ci`.

### Phase B — Presets

1. Add `subagents.presets` map in config (id → model, system_extra, tools_allowlist).
2. Validate `preset` param against allowlist in tool handler.
3. Tests for unknown preset rejection and allowlist-filtered tools.

### Phase C — External backends

1. Define `SubagentBackend` trait + registry keyed by `backend` string.
2. Implement **Cursor SDK backend** first (structured API, cancel support).
3. Implement **Claude Code CLI** and **Codex CLI** via sandbox executor (reuse exec patterns, longer timeout, structured stdout/stderr in capsule).
4. Mark external backends high-risk in `harness/policy.md`; require `enabled: true` in config.
5. Document env vars (`CURSOR_API_KEY`, etc.) in `config.example.yaml` only — no secrets in repo.

### Phase D — Async spawn (optional)

1. Add `spawn` tool returning immediate ack + task id.
2. On completion, inject synthetic tool result or user notification per channel policy.
3. Gateway/dispatcher hook for subagent completion queue.

## Validation

Phase A:

```bash
cargo fmt --all -- --check
cargo test -p bobaclaw-agent subagent tool_loop prompt
cargo test -p bobaclaw-core config
make ci
```

Phase C (when backends land):

```bash
cargo test -p bobaclaw-agent subagent backends
# Manual smoke with enabled backend in operator config.local.yaml (not committed)
```

Eval follow-up (not blocking Phase A):

- Add smoke scenario: parent calls `subagent` with a read-only task; assert structured result returned.

## Risks

| Risk | Mitigation |
|------|------------|
| Duplicate LLM cost (parent + child) | Lower child iteration cap; prompt guidance to delegate only when valuable |
| Context loss across delegation | Require self-contained `task` string; optional preset system snippets |
| External CLI escapes sandbox | Run via executor profiles; default `enabled: false`; approval for host-danger |
| Double API keys / billing | Separate env vars per backend; never inherit parent key silently |
| Session pollution | Ephemeral child messages only |
| Runaway concurrency | Semaphore + config `max_concurrent` |
| Refactor regression in `turn.rs` | Extract runner incrementally; keep existing turn tests green |

## Rollback plan

- Phase A: remove `subagent` from tool list and dispatch; revert `tool_loop` extraction if needed by inlining back into `turn.rs`; remove config keys (serde defaults keep old configs valid).
- Phase C: set all external backends `enabled: false` or remove backend modules; native subagent continues to work.
- No DB migration required (ephemeral child sessions).

## Completion notes

Fill after each phase:

- changed files:
- validation run:
- known gaps:
- follow-up work:

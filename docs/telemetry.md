# Telemetry for BobaClaw

Telemetry makes autonomous work reviewable. Goal: **reconstructability**, not surveillance.

## Runtime evidence (BobaClaw)

For every agent turn, the runtime captures:

| Source | What |
|--------|------|
| **Run Ledger** (`state.db`) | Run IDs, tool events, session linkage |
| **Command capsules** | `capsule.yaml`, script, stdout/stderr, `result.json` on disk |
| **AgentProgress** | Tool start/end events streamed to Telegram/CLI |
| **Turn metadata** | Iteration count, cancellation, compaction handoff |

Implementation: `crates/bobaclaw-state/src/ledger.rs`, `crates/bobaclaw-agent/src/progress.rs`, `crates/bobaclaw-executor/src/run.rs`.

## Trace events (conceptual)

- `task_started` / `turn_started`
- `tool_call_requested` / `tool_call_completed`
- `validation_failed` / `validation_passed`
- `turn_cancelled` / `turn_interrupted`
- `compaction_handoff`

## Log fields for tool calls

- tool name, run_id, duration, exit code;
- redacted input summary;
- stdout/stderr truncation status (full output in capsule);
- policy/executor profile decision.

Do not log raw secrets, tokens, or private user content.

## Repo harness evidence (Cursor agents)

For contributor agent runs, PRs should document:

- plan file;
- `make ci` output;
- changed files and rollback path.

## Metrics to track

- task success rate;
- tool failure rate;
- average iterations per turn;
- CI pass rate;
- eval regression rate;
- review correction rate.

## Retention

Run capsules and ledger rows follow operator retention policy on the homelab host. Redact before exporting logs.

# Agent change plan

## Goal

Make exec timeouts real: enforce a wall-clock limit on sandboxed commands, kill the subprocess on turn cancellation, and record `RunStatus::Timeout` in the run ledger.

## Context

Priority **P0 (reliability)** — part of the June 2026 reliability/autonomy review roadmap.

Findings:

- `timeout_secs: 120` is written to `capsule.yaml` only; both backends use blocking `cmd.output()` with no limit (`crates/bobaclaw-executor/src/bwrap.rs:53`, `crates/bobaclaw-executor/src/docker.rs:59`). A hung command (`tail -f`, interactive `read`) blocks a `spawn_blocking` thread forever.
- Turn cancellation (`/stop`, preempt by a new message) returns `TurnInterrupted` from the exec tool but does not kill the sandbox process (`crates/bobaclaw-agent/src/tools/exec.rs`); the command keeps running.
- `RunStatus::Timeout` exists in the ledger but is never set; `harness/tools/exec.md` claims the timeout is "bounded by executor" — contract drift.
- `ProfileKind::SystemdRun` silently falls back to bwrap on failure (`bwrap.rs:142`) — hidden behavior worth removing or making explicit while in this code.

## Scope

### In scope

- Spawn sandbox processes with a kill handle (process group); enforce `timeout_secs` (config-driven, default 120) with hard kill on expiry.
- Propagate `CancellationToken` into the executor so cancel kills the process group, not just the awaiting task.
- Set `RunStatus::Timeout` in the ledger and return a structured `exec timed out after Ns` tool body (exit code convention documented in `harness/tools/exec.md`).
- Make `timeout_secs` configurable per call (optional tool arg, capped by config max) and per config.
- Remove or make explicit the silent systemd-run→bwrap fallback.
- Update `harness/tools/exec.md` to match enforced behavior.

### Out of scope

- Parallel tool-call execution (tracked in `agent-loop-quality.md`).
- Docker container lifecycle changes beyond exec-level kill (`docker exec` process termination is enough).

## Files likely to change

- `crates/bobaclaw-executor/src/bwrap.rs`
- `crates/bobaclaw-executor/src/docker.rs`
- `crates/bobaclaw-executor/src/run.rs`
- `crates/bobaclaw-state/src/ledger.rs` (mark_timeout)
- `crates/bobaclaw-agent/src/tools/exec.rs`
- `crates/bobaclaw-core/src/config.rs` (executor timeout settings)
- `config.example.yaml`
- `harness/tools/exec.md`

## Implementation steps

1. Replace `cmd.output()` with spawned child + `wait_timeout`/async wait; kill process group on expiry; capture partial stdout/stderr.
2. Wire `CancellationToken` from the exec tool into the executor; on cancel, kill the child and mark the run interrupted (exit 130 stays).
3. Add `ledger.mark_timeout`; write `result.json` with timeout status.
4. Add config fields (`executor.timeout_secs`, `executor.max_timeout_secs`) and optional `timeout_secs` tool argument.
5. Remove/expose the systemd-run silent fallback.
6. Update harness contract; add tests: hung command times out, cancel kills the child (probe with `pgrep` after cancel).

## Validation

```bash
make ci
cargo test -p bobaclaw-executor -p bobaclaw-agent
```

Additional checks:

- Manual: `exec` of `sleep 600` returns a timeout body in ~120s; no orphan process remains.
- Manual: `/stop` during a long exec leaves no running sandbox process.

## Risks

- Killing process groups can terminate legitimate long builds; mitigate with per-call `timeout_secs` and a clear timeout message instructing the model to re-run with a longer budget.
- Partial-output capture semantics differ between bwrap and docker exec; cover both with tests.

## Rollback plan

Revert the branch; ledger gains only an additive status value, schema unchanged.

## Completion notes

Fill this after implementation:

- changed files:
- validation run:
- known gaps:
- follow-up work:

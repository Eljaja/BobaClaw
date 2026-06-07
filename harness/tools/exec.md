# Tool: exec

## Purpose

Run a shell command in the agent group workspace via the sandboxed executor. Primary tool for inspection, builds, git, scripts, and verification.

## Non-goals

- Do not use for gateway-side operations.
- Do not report stdout/stderr the tool did not return.
- Do not pass absolute host paths as `workdir`.

## Input schema

```json
{
  "type": "object",
  "properties": {
    "command": { "type": "string", "description": "Shell command (bash -lc)" },
    "workdir": { "type": "string", "description": "Subdir relative to workspace; omit for root" }
  },
  "required": ["command"]
}
```

## Output

Tool message body with exit code, stdout/stderr (truncated at 24k chars; full output in run capsule). Fields: `run_id`, `exit_code`.

## Side effects

- May write files in workspace.
- May use network if executor profile and config allow.
- Creates run capsule and ledger entries.
- May install packages when `sandbox_packages` enabled.

## Approval requirements

- Default sandbox: no extra approval.
- `host-danger` profile: operator approval required.
- Destructive commands outside workspace scope: prohibited.

## Timeouts and retries

- Bounded by executor/backend and turn cancellation token.
- Agent may retry after reading error; no automatic tool-level retry.
- Not idempotent unless command is.

## Failure modes

| Error | Agent response |
|-------|----------------|
| Empty command | Fix args; do not retry blindly |
| Invalid workdir | Use `.` or relative subdir |
| Sandbox policy denial | Diagnose with narrower command; suggest profile/backend to operator |
| Non-zero exit | Read stderr; fix and rerun |

## Telemetry

Run Ledger + capsule: command, workdir, profile, exit code, duration, truncation flag. Progress events: `ToolStart` / `ToolEnd`.

## Tests

`cargo test -p bobaclaw-agent`, `scripts/test-exec.sh`, `scripts/test-docker-sandbox.sh`.

Implementation: `crates/bobaclaw-agent/src/tools/exec.rs`.

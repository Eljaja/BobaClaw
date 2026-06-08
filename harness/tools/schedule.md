# Tools: schedule, schedule_recurring, schedule_list, schedule_cancel

## Purpose

Agent-managed scheduling (Hermes/OpenClaw style): one-shot and recurring jobs stored in SQLite, delivered to the originating chat (Telegram peer or CLI outbox).

## Non-goals

- Do not tell the user scheduling is impossible — use these tools.
- Do not edit `config.yaml` for agent-created recurring jobs (use `schedule_recurring`).
- `schedule_cancel` does not delete historical rows; it cancels pending one-shots or disables cron jobs.
- Not a host-level cron daemon replacement — jobs require a running BobaClaw process with scheduler enabled.

## Tools

| Tool | Use |
|------|-----|
| `schedule` | One-shot after `delay_seconds` |
| `schedule_recurring` | 5-field cron repeats |
| `schedule_list` | Pending one-shots + active cron jobs |
| `schedule_cancel` | Cancel `sched_*` or disable `cron_*` |

## Input: schedule

```json
{
  "type": "object",
  "properties": {
    "delay_seconds": { "type": "integer", "description": "1..604800 (7 days)" },
    "prompt": { "type": "string" },
    "deliver_message": { "type": "string", "description": "Fixed text; skips LLM at fire time" }
  },
  "required": ["delay_seconds", "prompt"]
}
```

## Input: schedule_recurring

```json
{
  "type": "object",
  "properties": {
    "cron": { "type": "string", "description": "5-field cron: min hour dom month dow" },
    "prompt": { "type": "string" },
    "deliver_message": { "type": "string", "description": "Fixed text; skips LLM each run" },
    "job_id": { "type": "string", "description": "Optional stable id (alphanumeric, dash, underscore)" }
  },
  "required": ["cron", "prompt"]
}
```

## Input: schedule_list

```json
{ "type": "object", "properties": {}, "required": [] }
```

## Input: schedule_cancel

```json
{
  "type": "object",
  "properties": {
    "id": { "type": "string", "description": "sched_* or cron_* id" }
  },
  "required": ["id"]
}
```

## Output

Plain-text confirmation (task/job id, run time or cron expr, deliver channel/peer) or list/cancel status. Not JSON.

## Side effects

- Inserts/updates rows in `scheduled_tasks` or `cron_jobs` (SQLite `state.db`).
- At fire time: may run a full agent turn (if no `deliver_message`) or deliver fixed text directly.
- Delivery: Telegram `sendMessage` or CLI outbox file under `~/.bobaclaw/outbox/`.
- Recurring jobs persist until `schedule_cancel` or operator DB edit.

## Approval requirements

- Within delay limits and normal chat scope: no approval.
- High-frequency or abusive scheduling: operator intervention via `schedule_cancel` or DB.
- Cron jobs that perform networked/destructive work at fire time follow normal `exec`/MCP policy at execution time.

## Timeouts and retries

- Scheduler tick: max `scheduler.tick_secs` (default 5s); adaptive wake for soon-due one-shots (min 1s).
- One-shot fires once; no automatic retry on delivery failure (task marked `failed`).
- Cron dedupes fires within tick window via `cron_runs`.
- `deliver_message` path: no LLM timeout (direct delivery).

## Failure modes

| Error | Agent response |
|-------|----------------|
| `delay_seconds` out of range | Use 1..604800 |
| Empty `prompt` | Provide actionable prompt |
| Invalid cron expression | Fix 5-field cron syntax |
| `schedule_cancel` id not found | Use `schedule_list`; verify prefix `sched_` / `cron_` |
| Telegram deliver without peer | Should not happen from routed chat; report misconfiguration |
| Scheduler not running | Inform operator: ensure `scheduler.enabled` and gateway/channel process is up |

## Telemetry

Stored in DB: task/job id, agent_group, session_id, channel, peer, run_at/cron, status, `last_error` on failure. Scheduler logs: `scheduled task … delivered`, `cron job … firing`.

## Tests and evals

```bash
cargo test -p bobaclaw-agent schedule::
cargo test -p bobaclaw-state
make ci
```

Regression scenarios (future): `evals/regression/scheduler-delivery.yaml`.

## Scheduler runtime

| Process | Scheduler |
|---------|-----------|
| `bobaclaw gateway start` | In-process when `scheduler.enabled` |
| `bobaclaw channel telegram start` | In-process when `scheduler.enabled` |
| `bobaclaw chat` | In-process when `scheduler.enabled` and `scheduler.embedded` |
| `bobaclaw scheduler start` | Foreground daemon (optional split deployment) |

Config YAML `cron.jobs` remains supported for operator-defined jobs with optional `deliver` block.

Implementation: `crates/bobaclaw-agent/src/tools/schedule.rs`, `crates/bobaclaw-scheduler/src/runner.rs`.

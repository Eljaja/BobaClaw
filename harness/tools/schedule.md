# Tool: schedule

## Purpose

Schedule a one-shot future task. At `run_at` the agent runs `prompt` and delivers the result to the user (Telegram or CLI outbox).

## Non-goals

- Not a cron/recurring scheduler (one-shot only in v1).
- Do not claim scheduling is impossible — use this tool.

## Input schema

```json
{
  "type": "object",
  "properties": {
    "delay_seconds": { "type": "integer", "description": "1..604800" },
    "prompt": { "type": "string" },
    "deliver_message": { "type": "string", "description": "Optional fixed delivery text" }
  },
  "required": ["delay_seconds", "prompt"]
}
```

## Output

Confirmation string with task id and scheduled time.

## Side effects

- Inserts row in `scheduled_tasks` (SQLite).
- Future turn consumes provider tokens and may send user-visible message.

## Approval requirements

- Within delay limits: no approval.
- Spammy or abusive scheduling: channel policy / operator intervention.

## Timeouts and retries

- Scheduler daemon fires once; agent turn at fire time subject to normal turn limits.
- Idempotent only if operator deduplicates tasks manually.

## Failure modes

| Error | Agent response |
|-------|----------------|
| delay out of range | Use 1..604800 seconds |
| empty prompt | Provide actionable prompt |
| scheduler not running | Inform operator to start `bobaclaw scheduler` |

## Telemetry

Task id, session, channel peer, run_at stored in DB. Delivery logged via channel adapter.

## Tests

`cargo test -p bobaclaw-agent`, scheduler integration via gateway.

Implementation: `crates/bobaclaw-agent/src/tools/schedule.rs`.

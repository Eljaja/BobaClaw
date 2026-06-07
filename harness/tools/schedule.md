# Tools: schedule, schedule_recurring, schedule_list, schedule_cancel

## Purpose

Agent-managed scheduling (Hermes/OpenClaw style): one-shot and recurring jobs stored in SQLite, delivered to the originating chat.

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
  "delay_seconds": { "type": "integer", "description": "1..604800" },
  "prompt": { "type": "string" },
  "deliver_message": { "type": "string", "description": "Fixed text; skips LLM at fire time" }
}
```

## Input: schedule_recurring

```json
{
  "cron": { "type": "string", "description": "5-field cron" },
  "prompt": { "type": "string" },
  "deliver_message": { "type": "string" },
  "job_id": { "type": "string", "description": "Optional stable id" }
}
```

## Scheduler runtime

- Runs in-process with `gateway start` and `channel telegram start` when `scheduler.enabled`.
- Optional foreground daemon: `bobaclaw scheduler start`.
- Adaptive sleep: wakes early for soon-due one-shot tasks.

## Side effects

- `scheduled_tasks` / `cron_jobs` rows in SQLite.
- Delivery via Telegram API or CLI outbox.

Implementation: `crates/bobaclaw-agent/src/tools/schedule.rs`.

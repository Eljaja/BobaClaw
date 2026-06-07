# Tool contract template

Use this template for every tool that the BobaClaw runtime agent can call. Copy to `harness/tools/<name>.md` and fill all sections.

## Tool name

`tool_name`

## Purpose

What the tool does and when the agent should use it.

## Non-goals

When the agent should not use this tool.

## Input schema

```json
{
  "type": "object",
  "properties": {},
  "required": []
}
```

## Output schema

```json
{
  "type": "object",
  "properties": {
    "ok": { "type": "boolean" },
    "result": {},
    "error": { "type": ["string", "null"] }
  },
  "required": ["ok"]
}
```

## Side effects

Declare all side effects:

- filesystem writes;
- network calls;
- repository writes;
- cloud resource changes;
- billing impact;
- persistent state changes;
- user-visible messages.

## Approval requirements

Specify whether approval is required for destructive, networked, credentialed, billing, repository-write, or user-data operations.

## Timeouts and retries

- timeout:
- retry count:
- retry backoff:
- idempotent: yes/no

## Failure modes

List expected errors and how the agent should respond.

## Telemetry

Log at minimum: tool name, run_id/trace id, duration, status, policy decision, redacted input/output summaries.

## Tests and evals

List required tests/evals before changing this tool.

Implementation reference: `crates/bobaclaw-agent/src/tools/`.

# Agent change plan

## Goal

Add a configurable per-turn tool-loop step limit (`agent.max_tool_iterations`) and raise the default from 16 to 60.

## Context

The agent tool loop in `turn.rs` used a hardcoded `MAX_TOOL_ITERATIONS = 16`. Operators need to tune this without rebuilding.

## Scope

### In scope

- New `AgentConfig` section in `BobaConfig`
- Wire limit into `run_agent_turn`
- Dynamic limit-reached user messages
- Update `config.example.yaml` and `docker/config.docker.yaml`
- Config load test

### Out of scope

- Prompt changes (`prompt.rs` stays free of magic numbers per AGENTS.md)
- Changing `MAX_ACTION_RETRIES` or `MAX_EMPTY_RESPONSE_RETRIES`

## Files likely to change

- `crates/bobaclaw-core/src/agent_config.rs` (new)
- `crates/bobaclaw-core/src/config.rs`
- `crates/bobaclaw-core/src/lib.rs`
- `crates/bobaclaw-agent/src/turn.rs`
- `config.example.yaml`
- `docker/config.docker.yaml`

## Implementation steps

1. Add `AgentConfig` with `max_tool_iterations` default 60.
2. Read limit from `config.agent` in `run_agent_turn`.
3. Update example configs and tests.
4. Run `make ci`.

## Validation

```bash
make ci
```

## Risks

- Very high values increase LLM cost and latency per turn.

## Rollback plan

Revert the commit; missing `agent` section falls back to default 60 via serde defaults.

## Completion notes

- changed files: `crates/bobaclaw-core/src/agent_config.rs`, `crates/bobaclaw-core/src/config.rs`, `crates/bobaclaw-core/src/lib.rs`, `crates/bobaclaw-agent/src/turn.rs`, `config.example.yaml`, `docker/config.docker.yaml`
- validation run: `make ci` — passed
- known gaps: none
- follow-up work: move plan to `plans/completed/` on merge

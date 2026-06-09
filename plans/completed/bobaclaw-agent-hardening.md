# Agent change plan

## Goal

Harden `bobaclaw-agent` per architecture review: remove brittle heuristics, centralize limits in config/core, add context-window guard, tool dispatch registry, prompt caching, and background post-turn review.

## Context

Review identified P0 fragilities (verb heuristic, duplicated magic numbers, hardcoded retries) and P1 improvements (ToolHandler, prompt cache, async review, subagent prompt files). Implementing P0 + P1 items 5–10 and context guard (P2 #11) in one scoped change.

## Scope

### In scope

- Remove `user_request_requires_tools`; use structural Parent + tools-offered signal
- `AgentConfig`: `max_action_retries`, `max_empty_response_retries`
- `bobaclaw-core`: `TOOL_BODY_PERSIST_MAX_CHARS`
- `ContextConfig`: `pre_call_compact_ratio` + guard before each LLM call
- `ToolHandler` registry replacing `run_tool_call` if-chain
- System prompt cache keyed by workspace mtimes + skill names
- Background `maybe_post_turn_review` via `tokio::spawn`
- Subagent prompt load from `~/.bobaclaw/prompts/subagent.md`
- `ToolLoopState` for interrupted outcomes
- Memory dir per-file cap (2 KB)

### Out of scope

- Mid-stream cancellation (`tokio::select!` on stream)
- Tool output TTL cache
- Prometheus metrics
- `active_turns` leak watchdog
- Extended thinking mode
- Removing `sanitize_user_reply` / leaked XML markers entirely

## Files likely to change

- `crates/bobaclaw-core/src/agent_config.rs`
- `crates/bobaclaw-core/src/context_config.rs`
- `crates/bobaclaw-core/src/limits.rs` (new)
- `crates/bobaclaw-core/src/lib.rs`
- `crates/bobaclaw-agent/src/turn.rs`
- `crates/bobaclaw-agent/src/tool_loop.rs`
- `crates/bobaclaw-agent/src/context.rs`
- `crates/bobaclaw-agent/src/compaction.rs`
- `crates/bobaclaw-agent/src/prompt.rs`
- `crates/bobaclaw-agent/src/loop_.rs`
- `crates/bobaclaw-agent/src/progress.rs`
- `crates/bobaclaw-agent/src/tools/router.rs` (new)
- `crates/bobaclaw-agent/src/tools/mod.rs`
- `config.example.yaml`

## Implementation steps

1. Add core constants and config fields with defaults + tests
2. Replace heuristic + wire config retries + shared persist limit
3. Context guard in tool loop + compaction helper
4. Tool router trait + migrate `run_tool_call`
5. Prompt cache, subagent file load, memory per-file cap
6. Background post-turn review; update `EmptyResponseRetry` event
7. Run `make ci`

## Validation

```bash
make ci
cargo test -p bobaclaw-agent
cargo test -p bobaclaw-core
```

## Risks

- Background review no longer appends save notices to user reply (faster turn; saves still happen)
- Structural `requires_action` may add extra nudges on informational Parent turns when tools are configured

## Rollback plan

Revert the branch; config new fields have serde defaults so old configs keep working.

## Completion notes

- changed files: `bobaclaw-core` (limits, agent_config, context_config), `bobaclaw-agent` (turn, tool_loop, context, compaction, prompt, loop_, progress, tools/router, subagent), `config.example.yaml`
- validation run: `cargo test -p bobaclaw-agent -p bobaclaw-core` — 76 passed; `cargo fmt --all` OK; full `make ci` fails on pre-existing `bobaclaw-executor` docker_mount macOS path tests (unrelated)
- known gaps: `sanitize_user_reply` / XML markers unchanged; mid-stream cancel, tool output cache, metrics, active_turns watchdog, thinking mode deferred; post-turn review no longer appends save notices to user reply (runs in `tokio::spawn`); structural `requires_action` may nudge informational Parent turns when tools are configured
- follow-up work: P2 items 12–15, 17–18; optional `TurnDeps` parameter object for `run_agent_turn`

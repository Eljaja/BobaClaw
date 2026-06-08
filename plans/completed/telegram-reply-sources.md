# Agent change plan

## Goal

Stop Telegram from leaking in-progress thinking text as the final reply (harness channel contract + code fix). Require browser-sourced answers to cite visited URLs in BobaClaw runtime prompts/workspace — not in harness tool docs.

## Context

Users reported inconsistent Telegram replies: some turns end with the BobaClaw progress header (`Thinking… (step 1)` plus partial assistant text) instead of a clean final answer. Root cause: `TelegramStream` spawns unbounded `editMessageText` updates that can race with `finalize_with_fallback`. Intermediate `AssistantChunk` events also polluted the progress log.

Separately, when the agent browses via Obscura MCP, answers should list **Sources** (URLs actually visited).

## Scope

### In scope

- Harness channel contract: `harness/channels/telegram.md`
- Telegram stream race fix + `AssistantChunk` filter in `TelegramStream`
- Runtime citation rule: `prompt.rs` `MCP_HINT`, `workspace-examples/home/TOOLS.md`
- Unit tests; `make ci` + `cargo fmt --check`

### Out of scope

- Harness `tools/mcp.md` citation text (runtime owns browser answer shape)
- New `web_search` / `web_fetch` tools
- Memory architecture RFC

## Files likely to change

- `harness/channels/telegram.md` (new)
- `harness/AGENTS.md`
- `crates/bobaclaw-channel-telegram/src/stream.rs`
- `crates/bobaclaw-agent/src/prompt.rs`
- `workspace-examples/home/TOOLS.md`
- `plans/active/telegram-reply-sources.md`

## Implementation steps

1. Document Telegram progress vs final reply in harness.
2. Guard `TelegramStream` finalize against stale progress edits; skip `AssistantChunk` in progress.
3. Add Sources rule to runtime MCP prompt and workspace `TOOLS.md`.
4. Run validation.

## Validation

```bash
make ci
cargo fmt --check
cargo test -p bobaclaw-channel-telegram
cargo test -p bobaclaw-agent prompt
```

## Risks

- Stale progress edits are dropped; user may see slightly older status until finalize.
- Citation rule is prompt/workspace until dedicated fetch tools exist.

## Rollback plan

Revert branch `fix/telegram-thinking-leak-and-sources`.

## Completion notes

- changed files: `harness/channels/telegram.md`, `harness/AGENTS.md`, `crates/bobaclaw-channel-telegram/src/stream.rs`, `crates/bobaclaw-agent/src/prompt.rs`, `workspace-examples/home/TOOLS.md`, `plans/active/telegram-reply-sources.md`
- validation run: `cargo fmt`, `cargo fmt --check`, `make ci` (exit 0)
- known gaps: citation rule is prompt/workspace-only until dedicated fetch tools; progress may lag one edit interval before finalize
- follow-up work: optional eval smoke entry for `harness/channels/telegram.md`

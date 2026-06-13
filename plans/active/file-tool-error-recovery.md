# Agent change plan

## Goal

Return file and web_fetch validation failures as tool results (exit_code 1) so the agent loop can recover instead of aborting the Telegram turn with a raw error.

## Context

User asked to summarize a YouTube URL; the model called `file_read` with an absolute/URL-like path, triggering `validate_relative_path` → fatal `Err` in `run_tool_loop` → `Error: path must be relative to the workspace` in Telegram. Exec and MCP already return errors as tool body text.

## Scope

### In scope

- FileHandler and WebFetchHandler: catch handler errors, return body + exit_code 1.
- `validate_relative_path`: detect http(s) URLs and clearer message for absolute paths.
- Tests for URL detection.

### Out of scope

- YouTube transcript tooling (yt-dlp skill / exec guidance).
- Enabling `web_fetch` in operator config.
- Memory/skill handler error recovery (same pattern, follow-up).

## Files likely to change

- `crates/bobaclaw-agent/src/tools/router.rs`
- `crates/bobaclaw-agent/src/tools/workspace_path.rs`
- `crates/bobaclaw-agent/src/tools/files.rs` (tool description hint)

## Validation

```bash
cargo test -p bobaclaw-agent
make ci
```

## Rollback plan

Revert the branch; behavior returns to fatal tool errors on validation failure.

# Channel: Telegram

## Purpose

Long-polling Telegram bot (`bobaclaw channel telegram start`) that maps one chat (DM, group, or forum thread) to one agent session and streams turn progress while the agent runs.

## Non-goals

- Do not expose raw tool stdout/HTML in progress edits.
- Do not send partial assistant prose as the final user-visible message.
- Do not change Telegram pairing/trust policy here (see `config.yaml` and gateway docs).

## Progress vs final reply

| Phase | Telegram behavior | User sees |
|-------|-------------------|-----------|
| Turn start | `sendMessage` placeholder | `BobaClaw` header + `Working…` |
| In progress | `editMessageText` on placeholder (throttled) | Short English status: `Thinking…`, `Running exec`, `Finished exec (ok)`, compaction |
| Turn end | `editMessageText` (or follow-up `sendMessage` if long) | **Final answer only** — formatted per `channels.telegram.format` (`html` or `plain`) |

Progress events come from `AgentEvent` via `TelegramStream` (`crates/bobaclaw-channel-telegram/src/stream.rs`).

### Events shown in progress

- `LlmThinking`
- `ToolStart` / `ToolEnd` (sanitized preview)
- `Compacting`
- `EmptyResponseRetry`
- `Interrupted`

### Events excluded from progress

- `AssistantChunk` — intermediate model text; must never appear in the progress log or overwrite the final message.

## Race-safety contract

`editMessageText` updates are asynchronous and throttled. Before replacing the placeholder with the final answer:

1. Set a `finalized` flag so no further progress edits apply.
2. Bump an edit generation counter so in-flight progress edits are dropped if they complete after finalize.

Violation symptom: user receives the progress header (`BobaClaw` + `Thinking… (step 1)` + partial text) as the lasting message instead of the final reply.

## Input / output

- **Inbound:** Telegram `getUpdates` → normalized text + optional media attachments.
- **Outbound:** Markdown/HTML-formatted text; split at Telegram UTF-16 limits.

## Side effects

- Sends/edits Telegram messages.
- May download media into the workspace sandbox.

## Approval requirements

- Bot token in config/env; DM pairing per operator policy.
- Group mentions / reply routing per `channels.telegram` config.

## Timeouts and retries

- `stream_edit_interval_ms` throttles progress edits (minimum 300 ms).
- Final message: retry with `TelegramFormat::Plain` if HTML parse fails.

## Failure modes

| Error | Expected behavior |
|-------|-------------------|
| HTML entity parse error | Retry final edit as plain text |
| Finalize edit fails | Log warning; placeholder may retain last progress line |
| LLM/provider error | Replace placeholder with error text |

## Telemetry

- `tracing::debug` on failed progress edits.
- `tracing::warn` on finalize failure after plain retry.

## Tests

```bash
cargo test -p bobaclaw-channel-telegram
```

Implementation: `crates/bobaclaw-channel-telegram/`.

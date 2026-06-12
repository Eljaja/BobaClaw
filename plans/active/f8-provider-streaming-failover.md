# Agent change plan: F8 — Provider streaming + failover

## Goal

Add SSE streaming for chat completions with Telegram/CLI delta delivery, and multi-provider failover with backward-compatible single-provider config.

## Context

- Provider is single OpenAI-compatible HTTP, non-streaming only (`docs/as-built.md`).
- Telegram already has `editMessageText` streaming UX.
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

1. **Streaming:** SSE in `bobaclaw-provider`; stream deltas to Telegram pipeline and CLI stdout; accumulate tool-call deltas per OpenAI spec.

2. **Failover:**
   ```yaml
   providers:
     - name: primary
       base_url: ...
       api_key: "{{secret:...}}"   # or api_key_env
       model: ...
       priority: 0
   ```
   - Retry on timeout, 429, 5xx → next provider.
   - Health state cached.
   - Metric: `bobaclaw_provider_failovers_total` (F3).

3. **Backward compatibility:** existing single-provider config remains valid.

### Out of scope

- Anthropic native protocol.
- Model routing by task type.

## Files likely to change

- `crates/bobaclaw-provider/`
- `crates/bobaclaw-agent/src/turn.rs` — stream handling, tool delta assembly
- `crates/bobaclaw-channel-telegram/src/stream.rs`
- `crates/bobaclaw-gateway/` — optional streaming on `/v1/chat/completions`
- `crates/bobaclaw-core/src/config.rs`
- `harness/tools/provider-failover.md`
- `docs/as-built.md`

## Implementation steps

1. SSE client and delta aggregation.
2. Wire streaming path to Telegram edit interval.
3. CLI stdout streaming mode.
4. Multi-provider config parsing (single-provider shim).
5. Failover loop with retry policy.
6. Health cache + failover metric.
7. Fake provider tests: 503 → second serves; streaming vs non-streaming parity.

## Validation

```bash
make ci
```

## Risks

- Partial tool-call JSON on stream interrupt.
- Double billing if failover retries non-idempotent calls (limit to chat completions).

## Rollback plan

- Single provider config unchanged; disable streaming in config if added flag.

## Dependencies

- F2 for `{{secret:}}` on provider keys (env still works).

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work:

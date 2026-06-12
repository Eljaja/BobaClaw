# Agent change plan: F1 — Approval flow for dangerous actions

## Goal

Implement operator approval for high-risk tool invocations using the existing `approvals` table, unblock `host-danger` only behind per-call approval, and pause/resume turns without holding the LLM HTTP connection.

## Context

- `approvals` table and `host-danger` profile are schema-only stubs (`docs/as-built.md`).
- `host-danger` currently bails in `crates/bobaclaw-executor/src/bwrap.rs`.
- Approvals are the #1 demanded feature in self-hosted agents post-OpenClaw security crisis.
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

1. **Risk classification** on tool invocations:
   - Source of truth: per-tool default + per-preset override in `config.yaml` (`tools.risk_overrides`).
   - `exec` with network = `medium`; `exec` with `host-danger` profile = `high`.
   - `schedule_*`, `memory_manage`, `skill_manage` write ops = `medium`.
   - Read-only tools = `low`.

2. **Config block:**
   ```yaml
   approvals:
     enabled: true
     require_for: [high]          # or [medium, high]
     timeout_seconds: 600
     on_timeout: deny             # deny | allow
   ```

3. **Approval flow:**
   - Tool call requires approval → insert row into `approvals` (`pending`, payload = tool name + args digest + session/scope).
   - Pause turn (persist turn state; do **not** hold LLM HTTP connection).
   - Notify via active channel:
     - **Telegram:** inline keyboard `✅ Approve` / `❌ Deny` (`callback_query` in `bobaclaw-channel-telegram`).
     - **CLI:** interactive y/n in `chat`; outbox + `bobaclaw approvals list/approve/deny` otherwise.
     - **Gateway:** `GET /api/approvals?status=pending`, `POST /api/approvals/{id}/approve|deny`.

4. **Outcomes:**
   - Approve → resume turn from persisted state, execute tool, continue loop.
   - Deny/timeout → inject tool result `{"error":"denied by operator"}`; model continues.

5. **`host-danger` unlock:** only when `approvals.enabled: true` + per-call approval. Without approvals it stays a bail.

### Out of scope

- Provenance-carrying approval UI (F14).
- Taint-forced approval (F5) — separate plan; design hooks only if needed.

## Files likely to change

- `crates/bobaclaw-agent/src/turn.rs` — pause/resume, risk gate
- `crates/bobaclaw-state/` — approvals CRUD, turn pause state
- `crates/bobaclaw-gateway/src/` — REST endpoints
- `crates/bobaclaw-channel-telegram/` — inline keyboard callbacks
- `crates/bobaclaw/` — CLI subcommands
- `crates/bobaclaw-executor/src/bwrap.rs` — host-danger path
- `crates/bobaclaw-core/src/config.rs` — `approvals`, `tools.risk_overrides`
- `migrations/` — if schema extensions needed
- `harness/tools/approvals.md` + contract test
- `docs/as-built.md`

## Implementation steps

1. Add risk enum and classification logic (tool defaults + config overrides).
2. Extend `approvals` usage: insert pending, list, approve/deny, audit decision timestamp.
3. Persist turn state at approval pause point (session/scope keyed).
4. Wire Telegram callback handler for approve/deny.
5. Add CLI `bobaclaw approvals list|approve|deny` and interactive prompt in `chat`.
6. Add gateway REST routes.
7. Implement resume path: reload state → execute approved tool → continue loop.
8. Implement deny/timeout path with synthetic tool error result.
9. Gate `host-danger` on `approvals.enabled` + approved call.
10. Add `bobaclaw doctor` check when approvals enabled (table reachable).
11. Harness: high-risk exec blocked → approved via gateway → completes.
12. Harness: deny → denied tool-result → turn finishes gracefully.

## Validation

```bash
make ci
```

Harness contract tests in `harness/tools/approvals.md`.

## Risks

- Turn state persistence bugs → duplicate or lost tool calls.
- Telegram callback race with timeout.
- Holding connections if pause/resume not decoupled from LLM HTTP.

## Rollback plan

- Set `approvals.enabled: false` (default off for new installs if following global rule).
- Revert migration if added; `host-danger` returns to bail.

## Dependencies

- None (foundation for F5, F13, F14, F15).

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work: F14 provenance UI; F5 taint-forced approval

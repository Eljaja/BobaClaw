# Agent change plan: F2 — Credential vault

## Goal

Ensure secrets never reach model context or sandbox env verbatim: encrypted vault, `{{secret:NAME}}` references, resolution only at executor/provider boundary, redaction everywhere else.

## Context

- Keys today live in env/config; external subagent backends export keys into sandbox (`docs/as-built.md` gaps).
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

1. **New crate `bobaclaw-vault`** (or module in `bobaclaw-core`):
   - Backend `file`: age/ChaCha20-encrypted file under `~/.bobaclaw/vault`.
   - Key from OS keyring via `keyring` crate; fallback `BOBACLAW_VAULT_KEY` env.

2. **CLI:** `bobaclaw secret set|get|list|rm <name>`.

3. **Reference syntax:** `{{secret:NAME}}` in config, MCP server env, subagent backends.

4. **Resolution boundary:** only at executor/provider spawn or HTTP call — immediately before use.

5. **Never write resolved values to:**
   - message history;
   - run ledger stdout capture (scrub with literal-match redaction);
   - logs;
   - compaction summaries.

6. **Tool result scrubbing:** literal secret values → `[REDACTED:NAME]`.

7. **Migrate `api_key_env`:** support both env and `{{secret:}}`.

### Out of scope

- Full credential proxy / one-shot tokens (future).
- UI for vault management beyond CLI.

## Files likely to change

- New: `crates/bobaclaw-vault/` (or `bobaclaw-core/src/vault/`)
- `crates/bobaclaw-provider/` — resolve before HTTP
- `crates/bobaclaw-executor/` — resolve before spawn env
- `crates/bobaclaw-mcp/` — resolve server env
- `crates/bobaclaw-agent/` — tool result scrubber
- `crates/bobaclaw/` — `secret` subcommands
- `crates/bobaclaw-core/src/config.rs` — secret ref parsing
- `harness/tools/vault.md` + contract tests
- `docs/as-built.md`

## Implementation steps

1. Implement encrypted file store + keyring integration.
2. CLI CRUD for secrets.
3. Config/env parser for `{{secret:NAME}}` (lazy, not at load into logs).
4. Hook resolution at provider HTTP client.
5. Hook resolution at executor env assembly.
6. Hook resolution at MCP subprocess env.
7. Redaction pass on tool results before persistence.
8. Redaction pass on run ledger stdout/stderr capture.
9. Redaction pass on compaction input/output.
10. Extend config schema for `api_key` / `api_key_env` / secret ref trinity.
11. Doctor: vault readable, keyring or env fallback works.
12. Acceptance tests: secret in exec env never in messages/run_events/capsules; echo tool redaction test.

## Validation

```bash
make ci
```

## Risks

- Redaction misses (substring, encoding) → secret leak.
- Keyring unavailable on headless/server → document `BOBACLAW_VAULT_KEY` fallback.

## Rollback plan

- Continue using `api_key_env` only; vault crate unused if no secrets stored.

## Dependencies

- None (F8 failover uses secret refs).

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work:

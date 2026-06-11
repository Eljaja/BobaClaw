# Agent change plan

## Goal

Make the gateway observable: a `/health` endpoint that actually checks subsystems, Prometheus metrics, and graceful shutdown — so an autonomous daemon stops being a black box.

## Context

Priority **P2 (operations)** — part of the June 2026 reliability/autonomy review roadmap. Complements `runtime-resilience-channels-scheduler.md` (supervisor needs a health signal to expose).

Findings:

- `/health` returns a static `"ok"` (`crates/bobaclaw-gateway/src/server.rs:110-112`); Docker healthchecks pass even when the DB is corrupt, the Telegram task is dead, or the scheduler stopped ticking.
- No `/metrics` endpoint or metrics crate anywhere, despite `docs/ARCHITECTURE.md` §6.10 naming `gateway_up`, `sessions_active`, `executor_run_duration`; deferred in `plans/completed/bobaclaw-agent-hardening.md`.
- Gateway has no graceful shutdown (no signal handler around `axum::serve`) and no `stop`/`status` CLI; only the scheduler daemon has a pidfile.
- Run ledger records no duration; `get_run` drops `executor_profile`.

## Scope

### In scope

- `/health` deep checks: SQLite ping, Telegram poll task liveness (heartbeat timestamp), scheduler last-tick timestamp, MCP server connection counts; JSON body with per-subsystem status, HTTP 503 on critical failure.
- `/metrics` (Prometheus text format): `gateway_up`, `turns_total{outcome}`, `turn_duration_seconds`, `active_turns`, `provider_retries_total`, `exec_runs_total{status}`, `exec_run_duration_seconds`, `telegram_poll_errors_total`, `scheduler_ticks_total`.
- Graceful shutdown on SIGTERM/SIGINT: stop accepting requests, cancel in-flight turn tokens, flush state.
- Record run duration + keep `executor_profile` in ledger reads.
- Keep `/health` and `/metrics` exempt from gateway auth (bind-address protected), consistent with `security-sandbox-env-gateway-auth.md`.

### Out of scope

- Grafana dashboards / alert rules (operator-local).
- Web UI / status page.
- Distributed tracing export (tracing stays log-based).

## Files likely to change

- `crates/bobaclaw-gateway/src/server.rs` (health, metrics, shutdown)
- `crates/bobaclaw-gateway/Cargo.toml` (metrics crate, e.g. `prometheus` or `metrics` + exporter)
- `crates/bobaclaw-agent/src/dispatcher.rs`, `loop_.rs` (turn metrics hooks)
- `crates/bobaclaw-channel-telegram/src/runtime.rs` (heartbeat)
- `crates/bobaclaw-scheduler/src/runner.rs` (tick timestamp)
- `crates/bobaclaw-state/src/ledger.rs` (duration, profile)
- `docker/` healthcheck tuning if response shape changes
- `docs/telemetry.md`

## Implementation steps

1. Subsystem heartbeat registry shared via gateway state; deep `/health` with JSON + status codes (keep plain `"ok"` body compatibility for existing Docker healthcheck or update compose).
2. Metrics registry + `/metrics` route; instrument dispatcher, exec tool, provider retries, Telegram poll loop, scheduler ticks.
3. Graceful shutdown: signal handler, axum `with_graceful_shutdown`, cancel active turn tokens.
4. Ledger duration + profile fix.
5. Update `docs/telemetry.md`; run validation.

## Validation

```bash
make ci
cargo test -p bobaclaw-gateway -p bobaclaw-state
```

Additional checks:

- Manual: kill the Telegram task → `/health` flips degraded within one heartbeat interval.
- Manual: `curl /metrics` shows counters increasing across a turn.

## Risks

- Deep health checks can flap under transient load; use last-success timestamps with generous thresholds rather than live probes per request.
- Changing `/health` response shape can break the existing Docker healthcheck — keep `200`/body contract or update compose in the same change.

## Rollback plan

Revert the branch; endpoints are additive, healthcheck contract preserved (or compose updated atomically).

## Completion notes

Fill this after implementation:

- changed files:
- validation run:
- known gaps:
- follow-up work:

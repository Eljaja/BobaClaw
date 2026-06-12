# Agent change plan: F3 — Prometheus metrics + OpenTelemetry tracing

## Goal

Add opt-in observability: Prometheus `/metrics` on gateway (and optional scheduler listener) plus OTLP tracing with GenAI semantic conventions.

## Context

- No metrics or tracing today (`docs/as-built.md`).
- Run ledger and turn loop provide natural instrumentation points.
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

1. **Metrics** (`observability.metrics.enabled`, off by default):
   - `metrics` + `metrics-exporter-prometheus` (or `prometheus` crate).
   - `GET /metrics` on gateway; optional standalone listener for scheduler daemon.

2. **Minimum metric set:**
   - `bobaclaw_turns_total{channel,agent_group,outcome}`
   - `bobaclaw_turn_duration_seconds{agent_group}`
   - `bobaclaw_llm_requests_total{model,outcome}` / `bobaclaw_llm_tokens_total{model,direction=prompt|completion}`
   - `bobaclaw_tool_calls_total{tool,outcome}` / `bobaclaw_tool_duration_seconds{tool}`
   - `bobaclaw_subagent_jobs{backend,state}` (gauge)
   - `bobaclaw_compactions_total`, `bobaclaw_approvals_total{decision}`

3. **Tracing** (`observability.tracing.endpoint`, off by default):
   - `tracing` + `tracing-opentelemetry`, OTLP exporter.
   - Root span per turn (`turn`); children: `llm_call`, `tool:<name>`, `compaction`, `subagent:<backend>`.
   - Propagate trace context into spawn jobs.
   - **OTel GenAI semantic conventions:** `gen_ai.system`, `gen_ai.request.model`, `gen_ai.usage.*`, tool-call attrs.

4. **Doctor:** metrics endpoint reachable; OTLP endpoint reachable if configured.

5. **Docs:** `docs/observability.md` with Jaeger verification steps + example Grafana dashboard JSON.

### Out of scope

- Full SaaS dashboard hosting.
- Custom non-OTel metric naming where GenAI conventions apply.

## Files likely to change

- `crates/bobaclaw-gateway/` — `/metrics` route
- `crates/bobaclaw-agent/` — turn/tool/LLM instrumentation
- `crates/bobaclaw-scheduler/` — optional metrics listener
- `crates/bobaclaw-core/src/config.rs` — `observability` block
- `crates/bobaclaw/` — doctor checks
- New: `docs/observability.md`
- Integration test: scrape `/metrics` after one turn
- `docs/as-built.md`

## Implementation steps

1. Add config defaults (all off).
2. Register Prometheus recorder and HTTP exporter.
3. Instrument turn start/end, outcomes.
4. Instrument LLM calls (tokens, model, outcome).
5. Instrument tool calls (duration, outcome).
6. Instrument compaction and approvals (when F1 lands).
7. Subagent job gauge from `spawn_jobs` / runtime.
8. Wire OTLP layer on `tracing` subscriber.
9. Add span hierarchy in turn loop.
10. Propagate context to subagent/spawn paths.
11. Doctor checks.
12. Integration test + observability doc.

## Validation

```bash
make ci
```

Integration test asserts counters increment after one turn.

## Risks

- Cardinality explosion on high-cardinality labels (tool names OK; avoid session IDs in labels).
- OTLP overhead when enabled on every turn.

## Rollback plan

- `observability.metrics.enabled: false`, `observability.tracing.endpoint: null`.

## Dependencies

- F1 metrics for `bobaclaw_approvals_total` (can stub until F1 merges).

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work: F12 watcher metrics; F4 budget breach counter

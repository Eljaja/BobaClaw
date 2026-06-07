# Evals for BobaClaw

Tests validate Rust code. Evals validate agentic **system** and **repository** behavior.

## Eval layers

### Smoke evals (PR-safe)

Structural checks via `make eval-smoke` and `evals/smoke/repository-contracts.yaml`:

- required harness files exist;
- tool/sandbox/policy contracts present;
- PR template asks for plan, validation, risk, rollback.

### Regression evals (future)

Scenario-based checks from past failures:

- sandbox policy changed without eval update;
- agent claims validation without evidence;
- plan scope drift vs final diff.

### Capability evals (future)

Realistic engineering tasks: add a tool following contract, fix failing test with docs/CI consistency.

## Eval design rules

- Keep PR smoke evals deterministic and fast; no external network in default CI.
- Store scenarios in-repo (`evals/smoke/*.yaml`).
- Prefer artifact checks (files, command output, trace events) over prose grading.
- Turn incidents into regression scenarios.

## Minimal scenario schema

See `evals/smoke/repository-contracts.yaml`.

## What to measure

- CI pass rate on first attempt;
- tool-call failure rate (Run Ledger);
- eval regressions;
- human review corrections;
- rollback frequency.

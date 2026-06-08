# Bootstrap agent-first harness (BobaClaw)

## Goal

Initialize BobaClaw as an agent-first repository with harness-engineering structure adapted from [Harness-engineering](https://github.com/Eljaja/Harness-engineering).

## Context

BobaClaw already had runtime Rust code, architecture docs, and a minimal root `AGENTS.md` focused on `prompt.rs`. This bootstrap adds the full contributor harness: plans, evals, tool/sandbox contracts, CI gates, Cursor rules, and docs for future agentic development on the codebase.

## Scope

### In scope

- root and nested `AGENTS.md` files;
- `.cursor/rules/` for harness workflow, Rust, prompt, contracts;
- docs: agent-first-repository, best-practices, ci-cd, evals, telemetry;
- harness templates and filled runtime tool contracts;
- smoke eval scenario;
- plan template and this bootstrap plan;
- PR template;
- `scripts/check_repo_structure.py`, `scripts/scan_secrets.py`;
- `.github/workflows/ci.yml`;
- `Makefile` targets: `ci`, `check-structure`, `scan-secrets`, `eval-smoke`.

### Out of scope

- model-based eval runner;
- changes to runtime agent behavior in `crates/`;
- deploy workflow modifications.

## Files changed

- `AGENTS.md`, `Makefile`, `README.md`
- `.cursor/rules/*`
- `docs/agent-first-repository.md`, `docs/best-practices.md`, `docs/ci-cd.md`, `docs/evals.md`, `docs/telemetry.md`
- `harness/**`, `evals/**`, `plans/**`
- `scripts/check_repo_structure.py`, `scripts/scan_secrets.py`
- `.github/pull_request_template.md`, `.github/workflows/ci.yml`

## Validation

```bash
make ci
```

## Risks

- Initial harness docs may need tightening as BobaClaw features grow.
- Smoke evals are structural only until regression scenarios are added from real incidents.

## Rollback plan

Revert the bootstrap commit(s) or delete added harness directories and restore prior `AGENTS.md` / `Makefile`.

## Completion notes

- changed files: see scope above;
- validation run: `make ci` (structure, secrets, fmt, clippy, tests);
- known gaps: no model-based eval runner; `evals/regression/` empty;
- follow-up work: add regression scenarios from agent failures; optional scheduled regression workflow.

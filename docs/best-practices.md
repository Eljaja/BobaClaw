# Harness engineering best practices

Harness engineering is the discipline of designing the environment around an AI agent so that useful autonomous work is repeatable, safe, inspectable, and measurable.

Applied to BobaClaw: the **runtime** (`crates/bobaclaw-agent`) executes user tasks; the **repo harness** (`AGENTS.md`, `plans/`, `harness/`, `evals/`, CI) governs how we safely evolve that runtime.

## 1. Treat the repository as an agent runtime

- Put durable operating rules in `AGENTS.md` (root + nested).
- Keep commands stable through `Makefile`.
- Make setup and validation executable: `make ci`, `make doctor`, `cargo test`.
- Keep local instructions close to risky directories (`harness/`, `evals/`, `plans/`).
- Keep plans and evals in the repo, not only in chat history.

## 2. Separate intent from implementation

Require a plan for non-trivial work. A good plan includes goal, scope, files, validation, risks, rollback, and completion notes.

## 3. Define tool contracts

Every runtime tool documents purpose, schema, side effects, approvals, timeouts, retries, telemetry, and failure modes. See `harness/tools/`.

## 4. Use sandboxes deliberately

Document filesystem, network, credentials, and resource limits. BobaClaw defaults: bubblewrap/Docker executor, workspace-scoped writes, Run Ledger audit. See `harness/sandbox-contract.md` and ADR 003.

## 5. Convert incidents into evals

When an agent or harness fails, add a regression scenario under `evals/` — not only a prompt tweak.

## 6. Keep CI boring and strict

Mechanical safety: structure contract, secret scan, fmt/clippy/tests, smoke evals. Logic lives in checked-in scripts, not only GitHub Actions YAML.

## 7. Make agent work reviewable

Reviewers should reconstruct: request → plan → diff → checks → risks → rollback. Use PR template, Run Ledger, and CI artifacts.

## 8. Put policy in code, not only prompts

Critical boundaries exist in executor profiles, pairing policies, tool validation, tests, evals, and CI — not only `prompt.rs`.

## 9. Prefer small reversible changes

Small PRs, explicit rollback, minimal diffs, no opportunistic refactors.

## 10. Measure the harness, not only the model

Track task success, tool failure rate, CI pass rate, eval regressions, review corrections, rollback frequency.

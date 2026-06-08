# Harness directory instructions

This directory defines the operational harness around the **BobaClaw runtime agent**: tool contracts, sandbox contracts, policies, and runtime boundaries.

Distinction:

- **Repo harness** (this directory) — documents and templates for contributors and evals.
- **Runtime enforcement** — `crates/bobaclaw-executor`, `crates/bobaclaw-agent/src/tools/`, pairing policy in config.

## Rules

- Write all files in this directory in **English**.
- Do not weaken root safety requirements in `AGENTS.md`.
- Any change to a tool, sandbox, prompt, or policy contract must update docs or evals when behavior changes.
- Prefer explicit contracts over prose-only guidance.
- Mark side effects, approval requirements, timeouts, retries, and telemetry fields.
- Treat sandbox, billing, credential, repository-write, and network capabilities as high-risk by default.

## Layout

| File | Purpose |
|------|---------|
| `tool-contract-template.md` | Template for new tools |
| `tools/*.md` | Filled contracts for shipped runtime tools |
| `channels/*.md` | Channel delivery contracts (Telegram, etc.) |
| `sandbox-contract.md` | Executor boundary model (ADR 003) |
| `policy.md` | Risk classes and approval rules |

## Required validation

Run from repository root:

```bash
make ci
```

For runtime tool changes, also run:

```bash
cargo test -p bobaclaw-agent
cargo test -p bobaclaw-executor
./scripts/test-exec.sh   # when executor behavior changes
```

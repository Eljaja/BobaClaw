# Agent-first repository architecture (BobaClaw)

An agent-first repository is designed so that coding agents can understand the project, make bounded changes, validate those changes, and hand them off for review without relying on hidden context.

BobaClaw is both a **Rust runtime** (`crates/`) and a **harness template** for future agentic development on this codebase.

## Canonical structure

```text
.
├── AGENTS.md                    # root operating contract
├── .cursor/rules/               # Cursor contributor rules
├── Makefile                     # stable local/CI entrypoints
├── README.md
├── crates/                      # Rust workspace (runtime)
├── docs/                        # architecture + harness guides
├── harness/                     # tool/sandbox/policy contracts
├── evals/                       # smoke/regression scenarios
├── plans/                       # reviewable task records
├── scripts/                     # integration + harness scripts
└── .github/
    ├── pull_request_template.md
    └── workflows/ci.yml
```

## Required components

### `AGENTS.md`

The operational contract for humans and Cursor agents editing the repo. Root file defines global workflow; nested files specialize `harness/`, `evals/`, `plans/`.

### `plans/`

Durable record of intent. Required for multi-file changes, CI/CD, harness/tool/sandbox changes, security work, and agent-generated diffs.

### `harness/`

Runtime boundaries for the BobaClaw agent: tool contracts (`harness/tools/`), sandbox rules, safety policy. Distinct from workspace `BOBACLAW.md` (end-user rules).

### `evals/`

Behavioral checks for repository and harness contracts. Complements `cargo test`.

### `.github/`

PR template and CI that mirror local `make ci` harness checks plus Rust quality gates.

## Nested instructions

The nearest `AGENTS.md` wins for local behavior but must not weaken root safety requirements.

## Agent-readable docs

Prefer concrete commands:

```text
Run `make ci` before handoff.
```

Avoid vague guidance like “make sure everything is good.”

## Language

Contributor instructions must be **English only**: `AGENTS.md`, `.cursor/rules/`, `harness/`, `evals/`, `plans/`, and harness guides in `docs/`. End-user workspace templates (`workspace-examples/`) may use other languages.

## Repository contract checklist

- [x] Root `AGENTS.md` exists.
- [x] `.cursor/rules/` with harness workflow and English-only rules.
- [x] Stable validation via `make ci`.
- [x] Plan template in `plans/templates/`.
- [x] PR template with validation and rollback sections.
- [x] Tool contracts for runtime tools (`exec`, `schedule`, skills, MCP).
- [x] Sandbox contract aligned with ADR 003 executor profiles.
- [x] Smoke eval suite in `evals/smoke/`.
- [x] CI runs structure check, secret scan, and Rust tests.

# AGENTS.md — BobaClaw repository operating contract

This file guides **humans and Cursor/dev agents** working on the BobaClaw codebase. It is **not** injected into the runtime agent — that uses workspace `BOBACLAW.md` under `~/.bobaclaw/workspace/<group>/`.

## Core rule

Prefer reproducible engineering over clever autonomous behavior. Every non-trivial change must be understandable from the request, plan, diff, validation output, and PR summary.

## Language

All **contributor instructions** in this repository must be written in **English**:

- root and nested `AGENTS.md`;
- `.cursor/rules/`;
- `harness/`, `evals/`, `plans/`;
- harness-engineering docs under `docs/` (`agent-first-repository.md`, `best-practices.md`, `ci-cd.md`, `evals.md`, `telemetry.md`);
- PR template and plan templates.

End-user runtime workspace files (`workspace-examples/`, `~/.bobaclaw/workspace/`) may use the operator's language. Runtime system prompt text in `prompt.rs` stays English for prompt-cache stability unless explicitly localized in a dedicated change.

## Required workflow

1. Read this file first.
2. Read the closest nested `AGENTS.md` before editing files in a subdirectory (`harness/`, `evals/`, `plans/`, `crates/`).
3. For multi-file, risky, or agent-generated changes, create or update a plan under `plans/active/` using `plans/templates/agent-change-plan.md`. When merged or finished, move it to `plans/completed/` with completion notes.
4. Keep changes small and scoped. Do not mix unrelated refactors with docs, policy, or CI changes.
5. Add or update tests/evals/checks when behavior, policy, or repository contracts change.
6. Run `make ci` before handoff when possible.
7. In the final summary or PR, include changed files, validation run, known gaps, and rollback path.

## Stable commands

```bash
make ci                 # harness checks + unit tests
make ci-full            # ci + fmt + clippy (full Rust gate)
make lint               # fmt-check + clippy + tests
make check-structure    # required harness files/directories
make scan-secrets       # lightweight secret-pattern scan
make eval-smoke         # smoke eval contract validation
make test               # cargo test --workspace
make build              # release binary
make doctor             # environment / sandbox probes
```

## Repository map

| Path | Role |
|------|------|
| `crates/` | Rust workspace — runtime, agent loop, executor, gateway, channels |
| `docs/` | Architecture, ADR, harness-engineering guides |
| `harness/` | Tool/sandbox/policy contracts for the runtime agent |
| `evals/` | Smoke and regression scenario definitions |
| `plans/` | Reviewable intent and implementation records |
| `scripts/` | Shell integration tests and harness validation (`make ci`); operator-local ad-hoc scripts are gitignored — see `.gitignore` patterns |
| `workspace-examples/` | End-user workspace templates (`BOBACLAW.md`, skills) |
| `references/` | Read-only Claw ecosystem snapshots (separate git repos) |
| `.cursor/rules/` | Cursor agent rules for repo contributors |

## Repository boundaries

Do not commit:

- API keys, tokens, passwords, SSH private keys, cookies, session data, or `.env` files.
- `config.local.yaml` (operator config with secrets and LAN-specific proxy URLs).
- Operator-local scripts matching `.gitignore` patterns under `scripts/` (private hosts, debug one-offs).
- Large generated artifacts (`target/`, build output) unless explicitly required.
- Vendor/cache directories such as `node_modules/`, `.venv/`, `.pytest_cache/`.
- Machine-local absolute paths in reusable docs or configs.
- Private LAN IPs or hostnames in tracked docs, examples, or scripts — use placeholders.

## Harness-engineering expectations

A high-quality agentic repository defines:

- **Instructions** — durable operational guidance in `AGENTS.md` (root + nested).
- **Plans** — reviewable intent in `plans/`.
- **Tool contracts** — schema, side effects, approval, timeout, retry, telemetry (`harness/tools/`).
- **Sandbox contracts** — filesystem, network, process, secrets boundaries (`harness/sandbox-contract.md`).
- **Evals** — deterministic smoke/regression checks (`evals/`).
- **CI/CD** — stable local commands mirrored in `.github/workflows/ci.yml`.
- **Telemetry** — traces and artifacts to reconstruct autonomous work (`docs/telemetry.md`, Run Ledger).

## Review stance

Treat agent output as untrusted until checks and human review validate it. A plan explains intent but does not prove correctness.

---

## BobaClaw-specific: runtime system prompt

The runtime system prompt is assembled in `crates/bobaclaw-agent/src/prompt.rs` via `build_system_prompt()`. It follows a **stable-tier** layout (Hermes-style) plus workspace bootstrap files (OpenClaw-style): identity, agent loop, tool discipline, compaction semantics, then optional `SOUL.md` / `BOBACLAW.md` / `MEMORY.md` from the user workspace.

### Do not add small implementation details to the prompt

Keep `prompt.rs` focused on **durable agent behavior**, not transient stack facts.

**Belongs in the prompt:** identity, agent loop, tool-use enforcement, task completion, memory/scheduling/skills/MCP *usage* rules, compaction handoff semantics, tone and language.

**Does not belong in the prompt:** executor backends (bubblewrap, Docker, flags), specific config keys that change often, sandbox mount paths, CLI subcommands, version-specific package paths, or other low-level internals. Put those in `docs/`, `config.example.yaml`, workspace `BOBACLAW.md` / `TOOLS.md`, or code — not in the cached system prompt.

When in doubt: if the detail is “how BobaClaw is built” rather than “how the agent should behave,” leave it out of `prompt.rs`.

### Agent loop best practices

The prompt must describe an **advanced agent loop**, not a single-shot Q&A:

1. **Observe → act → verify** — read tool output, decide what is still missing, call more tools until done.
2. **Multi-step autonomy** — chain `exec` / `schedule` / MCP across turns; do not stop after one command when the user asked for a deliverable.
3. **Failure recovery** — on error, diagnose with another tool call and try an alternative; do not invent stdout or exit codes.
4. **Parallelism** — independent reads/checks may be issued together when safe.
5. **Final answer discipline** — user-facing text only when work is complete or blocked; no plans or “I will run…” without a tool call in the same turn.

Runtime enforcement lives in `crates/bobaclaw-agent/src/turn.rs` (iteration limit, action nudges, empty-response retries). Prompt text should stay aligned with that loop, not duplicate magic numbers.

### Editing checklist

- Prefer short, stable English sections (prompt-cache friendly).
- Avoid duplicating the same rule in three constants — merge or cross-reference mentally.
- User- or deployment-specific rules → workspace `BOBACLAW.md`, not `prompt.rs`.
- After changes, run `cargo test -p bobaclaw-agent prompt`.

## Workspace vs repo

| File | Audience |
|------|----------|
| `AGENTS.md` (repo root) | Cursor / contributors editing BobaClaw |
| `workspace-examples/home/BOBACLAW.md` | Template for end-user agent workspace |
| `BOBACLAW.md` in `~/.bobaclaw/workspace/<group>/` | Injected into runtime agent context |

## Related docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — agent loop, executor, gateway
- [docs/agent-first-repository.md](docs/agent-first-repository.md) — harness layout for this repo
- [docs/best-practices.md](docs/best-practices.md) — harness engineering practices
- [docs/adr/003-executor-profiles.md](docs/adr/003-executor-profiles.md) — sandbox profiles (not for `prompt.rs`)

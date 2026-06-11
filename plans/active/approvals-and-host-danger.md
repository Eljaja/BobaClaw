# Agent change plan

## Goal

Implement the approval flow on top of the existing (currently dead) `approvals` table: the agent can request operator confirmation via Telegram/CLI for dangerous operations, unlocking a real `host-danger` path and approval-gated skill auto-promotion.

## Context

Priority **P2 (security / autonomy unlock)** — part of the June 2026 reliability/autonomy review roadmap. Do after `security-sandbox-env-gateway-auth.md` and `executor-timeout-and-cancel-kill.md`.

Findings:

- `approvals` table exists in `migrations/20260603100000_initial.sql` with no referencing code.
- `ProfileKind::HostDanger` bails with "requires explicit approval; not implemented" (`crates/bobaclaw-executor/src/bwrap.rs:78-81`); `readonly` profile is unreachable from config.
- Skill Forge promotion is operator-only (`draft_from_run` → manual `promote`); an approval primitive would allow safe agent-initiated promotion.
- Reference patterns: Hermes `agent/tool_guardrails.py` (command approval), nanoClaw OneCLI approvals.

## Scope

### In scope

- Approval state machine over the `approvals` table: `pending → approved | denied | expired` with TTL.
- Agent-side: when a tool call requires approval (host-danger exec, configurable command patterns), create an approval request, notify the operator (Telegram message with code / CLI instructions), and return a structured "awaiting approval" tool body instead of blocking the turn.
- Operator-side: `bobaclaw approvals list|approve|deny <id>`; optional Telegram inline reply ("approve <code>") from a paired operator peer.
- On approval: the pending run becomes executable on the next turn or via scheduler wake (reuse `SpawnCompleter`-style wake if cheap).
- Implement `host-danger` execution path gated strictly on an approved record; ledger marks `approved_by`/`approved_at`.
- Make the `readonly` profile reachable from config while touching profile selection.
- Approval-gated Skill Forge auto-promotion (agent proposes, operator approves, forge promotes).
- Harness/policy docs updated (`harness/policy.md`, `harness/sandbox-contract.md`, ADR note for executor profiles).

### Out of scope

- Full credential vault/proxy.
- Web UI for approvals.
- Auto-approval heuristics (everything defaults to manual).

## Files likely to change

- `crates/bobaclaw-state/src/` (approvals API)
- `crates/bobaclaw-executor/src/bwrap.rs`, `profile.rs` (host-danger + readonly selection)
- `crates/bobaclaw-agent/src/tools/exec.rs` (approval gate)
- `crates/bobaclaw/src/main.rs` (CLI subcommands)
- `crates/bobaclaw-channel-telegram/src/` (operator notification / approve reply)
- `crates/bobaclaw-skill-forge/src/forge.rs` (approval-gated promote)
- `crates/bobaclaw-core/src/config.rs` (approval policy config)
- `harness/policy.md`, `harness/sandbox-contract.md`, `docs/adr/003-executor-profiles.md`
- `config.example.yaml`, `migrations/` (only if schema needs columns)

## Implementation steps

1. State API + TTL expiry for approvals; unit tests for the state machine.
2. Exec approval gate: pattern config (`executor.approval_required` command globs + host-danger always); structured tool body with approval id.
3. CLI `approvals` subcommands; Telegram operator notification and approve-by-reply.
4. Host-danger execution path consuming approved records; ledger fields.
5. Readonly profile selection from config.
6. Skill Forge approval-gated promote.
7. Docs + validation.

## Validation

```bash
make ci
cargo test -p bobaclaw-state -p bobaclaw-executor -p bobaclaw-agent -p bobaclaw-skill-forge
```

Additional checks:

- Manual: request a host-danger exec → receive Telegram prompt → approve → command runs exactly once; deny → run marked denied.

## Risks

- Approval-by-Telegram-reply must be restricted to the paired operator peer; spoofing risk if group chats can approve — restrict to DM from allowlisted operator id.
- Host-danger is inherently dangerous; default config keeps it disabled even with the flow implemented.

## Rollback plan

Revert the branch; without code references the `approvals` table returns to dormant, host-danger returns to hard bail.

## Completion notes

Fill this after implementation:

- changed files:
- validation run:
- known gaps:
- follow-up work:

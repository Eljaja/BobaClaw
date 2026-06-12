# Agent change plan: F13 — Memory integrity (write validation, drift detection, rollback)

## Goal

Defend against memory poisoning / instruction drift: versioned gated writes to behavioural workspace files, write validation routed through approvals, drift detection, and operator rollback.

## Context

- Dominant real-world attack: slow corruption of `MEMORY.md`, `SOUL.md`, `BOBACLAW.md`, skills — not one-shot wire injection.
- **Co-priority with F1/F2.** Requires F1 (approval) and F5 (untrusted turn cannot write behavioural files).
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

1. **Versioned gated memory store.** Every write to `MEMORY.md`, `SOUL.md`, `memory/*`, `BOBACLAW.md`, skill files through single path:
   - Record content-addressed version (hash chain) in `memory_versions(id, file, hash, prev_hash, ts, source, turn_id, diff)`.
   - `source` ∈ `operator|agent|review|watcher`.
   - Untrusted-tainted turns (F5) **cannot** write behavioural files (`SOUL.md`, instruction-bearing sections of `BOBACLAW.md`) — operator or trusted turn only.
   - Untrusted content may only land in fenced, non-instruction memory regions.

2. **Write validation:** instruction-like content ("always run", "trust", "when you see X do Y", new trusted sources) → medium/high risk → F1 approval with provenance (F14).

3. **Drift detection:** periodic budgeted check (`review` scope): compare behavioural files vs signed baseline + recent diffs; flag new persistent rules, newly trusted sources, instruction-like additions from untrusted provenance → operator alert only.

4. **Rollback:** `bobaclaw memory log|diff <v1> <v2>|restore <file> <version>` — operator-only; N trusted checkpoints.

5. **Weekly review digest:** optional scheduled prompt listing persistent instructions and trusted sources for human review.

### Out of scope

- Automatic rollback on drift (alert only).
- Agent-initiated restore.

## Files likely to change

- `migrations/` — `memory_versions`
- `crates/bobaclaw-agent/` — gated `memory_manage`, skill writes
- `crates/bobaclaw-skills/`
- `crates/bobaclaw/` — `memory` subcommands
- `crates/bobaclaw-scheduler/` — drift check + weekly digest jobs
- `harness/tools/memory-integrity.md`
- `docs/as-built.md`

## Implementation steps

1. Migration + hash chain writer on every memory/skill file mutation.
2. Centralize all workspace behavioural writes through gate.
3. F5 integration: block untrusted turns from behavioural paths.
4. Instruction-like content classifier → F1 approval queue.
5. Baseline signing / storage on operator `init` or first run.
6. Drift job: diff + heuristics + alert channel.
7. CLI log/diff/restore with hash verification.
8. Optional weekly scheduler digest.
9. Acceptance: untrusted append blocked/approved; restore reverts poison; drift flags injected trusted source.

## Validation

```bash
make ci
```

## Risks

- False positives on legitimate MEMORY updates → approval fatigue.
- Baseline loss on restore → document checkpoint policy.

## Rollback plan

- Bypass gate flag for emergency (operator-only config, off by default).

## Dependencies

- **F1** (approval for risky writes).
- **F5** (taint gate for behavioural files).
- Soft: F14 provenance in approval payload.

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work:

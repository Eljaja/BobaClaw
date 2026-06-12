# Agent change plan: F9 — `bobaclaw onboard` wizard

## Goal

Interactive TTY wizard for first-time setup: provider, vault secret, sandbox detection, Telegram pairing, budgets, approvals — writes `config.yaml` and runs `doctor`.

## Context

- No onboard flow today (`docs/as-built.md` scaffolding).
- Best after F1, F2, F4 so wizard can configure real security/budget surfaces.
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

- Interactive wizard (`dialoguer` / similar): provider URL, model, API key → vault (F2).
- Sandbox backend detection (bwrap/docker).
- Telegram bot token + pairing smoke test.
- Budget defaults (F4).
- Approvals on/off (F1).
- Write `~/.bobaclaw/config.yaml` from template.
- Run `bobaclaw doctor` at end; print next steps.

### Out of scope

- Web-based setup.
- Remote gateway provisioning.

## Files likely to change

- `crates/bobaclaw/src/` — new `onboard` subcommand
- `workspace-examples/` — reference config fragments
- `docs/as-built.md`, README

## Implementation steps

1. Wizard flow module with skip/back.
2. Provider step → vault secret store.
3. Sandbox doctor probes inline.
4. Telegram token + test message optional step.
5. Budget/approval toggles with sane defaults.
6. Config render from answers.
7. Invoke doctor; exit code reflects health.

## Validation

```bash
make ci
# Manual: bobaclaw onboard in fresh HOME
```

## Risks

- Non-TTY environments — detect and print manual config guide instead.

## Rollback plan

- Wizard is additive CLI only.

## Dependencies

- F1, F2, F4 should land first (wizard configures them).

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work:

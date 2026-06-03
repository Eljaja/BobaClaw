# ADR 004: Skill Forge

**Status:** accepted  
**Date:** 2026-06-03

## Context

Hermes treats skills as the preferred extension mechanism with guard scanning and agent-created drafts.

## Decision

- Skills live in `workspace/<group>/skills/<name>/SKILL.md` (Hermes-compatible frontmatter).
- Staging: `skills-staging/<draft-id>/` with `provenance.json`, `manifest.yaml`.
- **Skill Forge** sources: successful runs, capsules, operator `draft-from-run`.
- Promotion requires guard pass (`agent-created` policy) and operator `bobaclaw skills promote`.
- No auto-activation without approval in v1.

## Trust levels (guard)

- `builtin`, `trusted`, `community`, `agent-created` — same policy shape as Hermes `skills_guard.py`.

## Commands

- `bobaclaw skills list|view|guard|draft-from-run|promote`

## Consequences

- Semi-auto suggestions (cron daemon) deferred to phase 2.

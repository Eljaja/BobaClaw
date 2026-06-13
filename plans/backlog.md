# BobaClaw Feature Backlog

**Purpose:** ordered task specs for coding agents. Implement top to bottom unless a plan explicitly notes a dependency override.

**Architecture baseline:** read `docs/as-built.md` before any feature work (13-crate Rust workspace, SQLite WAL `state.db`, gateway on axum, Telegram channel, bwrap/docker executor).

---

## Global rules (every feature)

- Read `crates/` structure and `docs/as-built.md` before writing code. Do not invent modules that already exist.
- Every feature ships with:
  - migration (if DB touched);
  - config keys with defaults;
  - `bobaclaw doctor` check (if it adds external surface);
  - harness contract test in `harness/tools/`;
  - section appended to `docs/as-built.md`.
- No feature may widen the attack surface silently: anything that adds network listeners, secrets handling, or new tool capabilities must be **off by default** in `config.yaml`.
- Keep changes per-feature in a **single PR/branch**. Do not bundle.

---

## Implementation order

| Order | ID | Plan | Priority | Depends on |
|------:|----|------|----------|------------|
| 1 | F1 | [f1-approval-flow](active/f1-approval-flow.md) | P0 | — |
| 2 | F2 | [f2-credential-vault](active/f2-credential-vault.md) | P0 | — |
| 3 | F13 | [f13-memory-integrity](active/f13-memory-integrity.md) | P0.5 | F1, F5 (taint gate for behavioural writes) |
| 4 | F5 | [f5-taint-injection-policy](active/f5-taint-injection-policy.md) | P1 | F1 |
| 5 | F14 | [f14-provenance-approvals](active/f14-provenance-approvals.md) | P0.5 | F1, F5; F12 for watcher traces |
| 6 | F3 | [f3-prometheus-otel](active/f3-prometheus-otel.md) | P0 | — |
| 7 | F4 | [f4-cost-budgets](active/f4-cost-budgets.md) | P0 | F3 (metrics) optional |
| 8 | F15 | [f15-circuit-breakers](active/f15-circuit-breakers.md) | P0.5 | F1, F4, F5 |
| 9 | F6 | [f6-history-search](active/f6-history-search.md) | P1 | F5 (taint markers in snippets) |
| 10 | F7 | [f7-event-triggers](active/f7-event-triggers.md) | P1 | F5 (payload taint) |
| 11 | F8 | [f8-provider-streaming-failover](active/f8-provider-streaming-failover.md) | P1 | F2 (`{{secret:}}` for keys) |
| 12 | F12 | [f12-watcher](active/f12-watcher.md) | P1.5 | **F5 required**; F4, F3, F7 (refactor) |
| 13 | F16 | [f16-notification-triage](active/f16-notification-triage.md) | P0.5 | F12, F15, F4 |
| 14 | F9 | [f9-onboard-wizard](active/f9-onboard-wizard.md) | P2 | F1, F2, F4 |
| 15 | F10 | [f10-systemd-packaging](active/f10-systemd-packaging.md) | P2 | — |
| 16 | F11 | [f11-minimal-tui](active/f11-minimal-tui.md) | P2 | F1, F3, F4 (gateway API) |

### Priority notes

- **F13 is co-priority with F1/F2:** memory poisoning / instruction drift is the primary post-OpenClaw attack vector; BobaClaw's persistent workspace markdown is the targeted surface.
- **F12 may be pulled up** after F1/F2/F5 if proactivity is the current product focus. Hard dependency: **F5 (taint) must land before or together with F12**.
- **F7 is a minimal precursor to F12** — keep it small; webhook and file watcher are later refactored into Watcher `PushSource`/`PollSource`.

---

## Defense-in-depth layer

Point solutions fail in isolation. Treat these as one coherent lifecycle-aware policy layer, not independent bolt-ons:

| Phase | Features |
|-------|----------|
| Pre-action | F1 (approvals), F2 (vault), F5 (taint) |
| In-action | F13 (memory integrity), F14 (provenance), F15 (circuit breakers) |
| Post-action / ops | F3 (observability), F4 (budgets), F16 (reliability + notification triage) |
| Proactivity | F7 → F12 (watcher), F16 (triage) |

---

## Channel depth (Telegram groups)

Not part of the F1–F16 ordered backlog, but tracked for parity with Hermes/OpenClaw group UX:

| Plan | Priority | Notes |
|------|----------|-------|
| [telegram-group-behavior-hermes-openclaw](active/telegram-group-behavior-hermes-openclaw.md) | P1 → P2 | Observe mode, `group_allow_from`, `mention_patterns`, per-group overrides, `/activation`; Phase 2 needs F5 taint |

Current gap: `allowed_groups: []` denies all groups; no observe transcript, no sender gate in groups, no wake words.

---

## Explicit non-goals

Do not implement unless re-prioritized:

- Channel breadth race (Discord/Slack/WhatsApp) — commodity, low differentiation.
- Web UI / control panel — TUI (F11) covers the operator loop.
- Skill marketplace/registry — security liability; local skills + guard audit only.
- Anthropic-native protocol — OpenAI-compatible surface is sufficient for now.
- OpenAGI-style observation daemon / screen watching — Watcher (F12) with explicit sources is the deliberate alternative.

---

## Suggested order rationale

F1+F2 close the security gap the market punishes hardest. F13 (memory integrity) is co-priority: post-OpenClaw research identifies memory poisoning — not wire-level injection — as the primary attack vector. F14 makes approvals defensible by carrying provenance; F15 contains cascading failure; both compose on F1/F5. F3+F4 are differentiators (observability + budgets) and leverage existing run-ledger plumbing. F5 composes on F1. F6 is near-free. F7 delivers minimal proactivity and is later absorbed by F12. F8 fixes single-point-of-failure provider. F12 is the proactivity flagship: main engineering investment is rules/triage, not connectors. F16 makes long-running reliability and proactive notification trustworthy — without it, the Watcher gets muted and silent failures erode trust. P2 is polish.

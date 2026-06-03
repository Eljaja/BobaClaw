# ADR 001: Rust Runtime Stack

**Status:** accepted  
**Date:** 2026-06-03

## Context

BobaClaw needs a self-hosted ChatOps agent with gateway, executor isolation, Hermes-like state, and skills.

## Decision

- **Language:** Rust (edition 2021), workspace with focused crates.
- **Async runtime:** `tokio`
- **HTTP gateway:** `axum`
- **CLI:** `clap`
- **Config:** YAML (`serde_yaml`) + env for secrets
- **State:** SQLite via `sqlx` with WAL (`~/.bobaclaw/state.db`)
- **LLM:** OpenAI-compatible HTTP (`reqwest`)
- **Tracing:** `tracing` + `tracing-subscriber`

## Crate boundaries

| Crate | Responsibility |
|-------|------------------|
| `bobaclaw` | CLI binary |
| `bobaclaw-core` | Config, paths, shared types |
| `bobaclaw-state` | SQLite sessions, messages, ledger |
| `bobaclaw-provider` | LLM client |
| `bobaclaw-executor` | bubblewrap / systemd-run profiles |
| `bobaclaw-agent` | Agent loop |
| `bobaclaw-gateway` | REST + `/v1/chat/completions` |
| `bobaclaw-skills` | SKILL.md registry + guard |
| `bobaclaw-skill-forge` | Draft/promote skills from runs |

## Consequences

- Single static binary deployment on homelab/WSL.
- No Docker required for v1 executor default (bubblewrap).
- References in `references/` remain read-only; not linked at compile time.

# ADR 002: State DB and Run Ledger

**Status:** accepted  
**Date:** 2026-06-03

## Context

BobaClaw requires auditable, replayable execution. Hermes uses `~/.hermes/state.db` for sessions, messages, and FTS.

## Decision

- Primary structured store: **`~/.bobaclaw/state.db`** (SQLite, WAL, foreign keys).
- Large blobs on disk under `~/.bobaclaw/runs/<run_id>/`.
- **Run Ledger** is source of truth for execution outcomes; user-facing status derives from ledger + executor confirmation.

## Schema (v1)

- `sessions`, `messages`, `messages_fts`
- `runs`, `run_events`
- `approvals`, `routes`
- `cron_jobs`, `cron_runs`
- `skill_drafts`, `schema_version`

## Run lifecycle

`created` → `script_saved` → `approved` (optional) → `started` → `completed` | `failed` | `timeout` | `denied`

Events appended to `run_events` with optional JSON payload.

## Consequences

- Concurrent gateway + CLI safe via WAL (Hermes pattern).
- Migrations in `migrations/001_initial.sql`.
- JSONL export possible later; not primary store.

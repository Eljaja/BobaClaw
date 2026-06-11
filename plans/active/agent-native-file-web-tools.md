# Agent change plan

## Goal

Add native `file_read` / `file_write` / `file_edit` and `web_fetch` tools so the agent stops paying token and fragility costs for `cat`/`sed`/`curl` through `exec`.

## Context

Priority **P1 (autonomy toolset)** — part of the June 2026 reliability/autonomy review roadmap. Pattern proven in NullClaw (`src/tools/file_*.zig`, `web_fetch.zig`) and PicoClaw (`pkg/tools/integration/web.go`); listed as a P1 gap in `docs/features.md`.

Findings:

- All file operations go through sandboxed shell: expensive escaping, no structured errors, whole-file rewrites via heredoc, every edit audited only as an opaque shell command.
- Web access requires either `exec curl` (sandbox network permitting) or a configured browser MCP server — heavyweight for "fetch this URL as text".

## Scope

### In scope

- `file_read(path, offset?, limit?)` — workspace-relative paths only; standard head/tail truncation.
- `file_write(path, contents)` — create/overwrite inside the workspace.
- `file_edit(path, old_string, new_string, replace_all?)` — exact string replacement with uniqueness check.
- `web_fetch(url)` — host-side HTTP GET with size cap, timeout, content-type allowlist (text/html/json), HTML-to-text reduction; respects a config kill switch (`tools.web_fetch.enabled`).
- Path traversal guards (no `..`, no absolute paths, no symlink escape from workspace).
- Run-ledger entries for file writes/edits (auditability on par with exec).
- Harness contracts in `harness/tools/` and short prompt hints in `prompt.rs`.

### Out of scope

- `web_search` (needs a search provider/API decision — follow-up).
- Browser automation (stays MCP/Obscura).
- File tools for paths outside the agent workspace.

## Files likely to change

- `crates/bobaclaw-agent/src/tools/` (new `files.rs`, `web.rs`; `specs.rs`, `router.rs`, `mod.rs`)
- `crates/bobaclaw-agent/src/prompt.rs` (exec-discipline hint update)
- `crates/bobaclaw-core/src/config.rs` (web_fetch config)
- `crates/bobaclaw-state/src/ledger.rs` (optional file-op run records)
- `config.example.yaml`
- `harness/tools/files.md`, `harness/tools/web_fetch.md` (new)
- `evals/` (smoke: edit a file via file_edit; fetch a known URL)

## Implementation steps

1. Implement path-sanitized file tools with tests for traversal/symlink escapes.
2. Wire ledger records for mutating file ops.
3. Implement `web_fetch` with reqwest: timeout, max bytes, redirect cap, text extraction; config gate.
4. Tool specs + router + prompt hint ("prefer file tools over cat/sed via exec").
5. Harness contracts + eval scenarios.
6. Run validation.

## Validation

```bash
make ci
cargo test -p bobaclaw-agent
make eval-smoke
```

Additional checks:

- Manual: ask the agent to edit a config value in a workspace file; verify it uses `file_edit` and the diff is exact.

## Risks

- `web_fetch` runs on the host (outside sandbox) — SSRF risk; mitigate with private-IP/localhost denylist and config kill switch.
- File tools bypass the exec sandbox; they must stay strictly workspace-scoped, mirroring the workspace mount exec already has.
- Model may oscillate between `exec` and file tools; prompt hint plus tool descriptions need one clear preference rule.

## Rollback plan

Revert the branch; tools are additive and config-gated.

## Completion notes

Fill this after implementation:

- changed files:
- validation run:
- known gaps:
- follow-up work:

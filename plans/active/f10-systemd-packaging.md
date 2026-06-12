# Agent change plan: F10 — systemd units + packaging

## Goal

Ship `bobaclaw install systemd [--user]` generating hardened units for gateway and scheduler daemon, documented in README.

## Context

- No systemd packaging today (`docs/as-built.md`).
- Operators on Linux homelab expect user/system units with sandbox hardening.
- **Baseline:** `docs/as-built.md`.

## Scope

### In scope

- `bobaclaw install systemd [--user]`:
  - Units for `gateway start` and `scheduler start`.
  - Hardening: `ProtectSystem=strict`, `NoNewPrivileges=true`, `PrivateTmp=true`, restricted `ReadWritePaths` to `~/.bobaclaw`.
  - Environment file reference for secrets (vault/keyring documented).
- README section: enable, start, logs, upgrade.

### Out of scope

- Debian/RPM packages (future).
- macOS launchd (separate if needed).

## Files likely to change

- `crates/bobaclaw/src/` — install subcommand
- `templates/systemd/` — unit templates
- `README.md`, `docs/as-built.md`

## Implementation steps

1. Unit templates with placeholders for binary path and user.
2. `--user` vs system target paths.
3. Hardening directives validated against common Ubuntu systemd.
4. Install writes units + daemon-reload hint (not auto-reload as root without flag).
5. README documentation.

## Validation

```bash
make ci
# Manual: systemd --user dry-run on WSL/Linux
```

## Risks

- `ProtectSystem=strict` breaks custom workspace paths outside home — document overrides.

## Rollback plan

- `bobaclaw uninstall systemd` removes generated units (optional companion command).

## Dependencies

- None.

## Completion notes

- changed files:
- validation run:
- known gaps:
- follow-up work:

# ADR 003: Executor Profiles

**Status:** accepted  
**Date:** 2026-06-03

## Context

Tools must not run on the gateway process. Default isolation should be lightweight on Linux homelab.

## Decision

Executor profiles (v1):

| Profile | Backend | Notes |
|---------|---------|-------|
| `bwrap-default` | bubblewrap | Default; no network |
| `bwrap-networked` | bubblewrap | `--share-net` when allowed |
| `readonly` | bubblewrap | read-only root binds |
| `systemd-run` | systemd-run | Falls back to bwrap if unavailable |
| `host-danger` | host shell | Requires explicit approval; never default |

Every execution:

1. Saves script + `capsule.yaml` before run.
2. Records Run Ledger events.
3. Captures stdout, stderr, exit code, `result.json`.

## Consequences

- `bobaclaw doctor` probes bwrap user namespaces.
- WSL may deny namespaces; runs get policy denial unless profile changed.

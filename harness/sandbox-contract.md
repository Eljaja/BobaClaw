# Sandbox contract (BobaClaw)

Defines boundaries for the BobaClaw **runtime agent** executing commands via the executor. Implementation: ADR 003, `crates/bobaclaw-executor/`, `config.example.yaml`.

## Boundary model

| Dimension | BobaClaw default | Config / profile |
|-----------|------------------|------------------|
| **filesystem** | Scoped workspace writes | Group workspace under `~/.bobaclaw/workspace/<group>/` |
| **network** | Off by default (`bwrap-default`) | `executor.network: true`, `bwrap-networked`, or Docker sandbox |
| **process execution** | Sandboxed bash in executor | Never on gateway process |
| **credentials** | Provider key in gateway env only | Not injected into sandbox by default |
| **persistence** | Workspace persisted; capsules per run | `~/.bobaclaw/runs/<run_id>/` |
| **resource limits** | Executor/backend dependent | Docker image, bwrap namespaces |

## Executor profiles (v1)

| Profile | Backend | Notes |
|---------|---------|-------|
| `bwrap-default` | bubblewrap | Default; no network |
| `bwrap-networked` | bubblewrap | `--share-net` when policy allows |
| `readonly` | bubblewrap | read-only root binds |
| `systemd-run` | systemd-run | Falls back to bwrap |
| `host-danger` | host shell | Explicit approval only; never default |

Every execution:

1. Saves script + `capsule.yaml` before run.
2. Records Run Ledger events.
3. Captures stdout, stderr, exit code, `result.json`.

## Recommended default (untrusted input)

- scoped workspace writes only;
- no host filesystem outside binds;
- no credentials in sandbox;
- no network unless task requires it and policy allows;
- bounded output to model (head/tail); full log in capsule;
- `bobaclaw doctor` probes bwrap user namespaces (WSL may deny).

## Approval triggers

Require explicit operator approval before:

- `host-danger` profile;
- deleting files outside task scope;
- changing executor profiles or sandbox code;
- opening network on locked-down deployments;
- writing to external repos or production hosts.

## Observability

Capture per run:

- command, workdir, executor profile;
- exit code, duration;
- stdout/stderr truncation status;
- capsule path;
- ledger events.

## Failure handling

When a sandbox command fails, the agent should:

1. read stderr/exit code from tool result (not invent output);
2. make the smallest plausible fix;
3. rerun the narrow command;
4. stop after repeated failures and summarize evidence.

WSL namespace denial → suggest Docker backend or profile change via operator, not silent fallback to host.

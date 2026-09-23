# Sandbox contract (BobaClaw)

Defines boundaries for the BobaClaw **runtime agent** executing commands via the executor. Implementation: ADR 003, `crates/bobaclaw-executor/`, `config.example.yaml`.

## Boundary model

| Dimension | BobaClaw default | Config / profile |
|-----------|------------------|------------------|
| **filesystem** | Scoped workspace writes | Group workspace under `~/.bobaclaw/workspace/<group>/` |
| **network** | **On** by default (`executor.network: true` → `bwrap-networked` / `docker-networked`) | Set `executor.network: false` for `bwrap-default` / `docker-default` (no egress) |
| **process execution** | Sandboxed bash in executor | Never on gateway process |
| **environment** | Cleared: bwrap `--clearenv` + fixed `PATH`, `HOME`, `LANG`, `TERM` (+ `TMPDIR`, `APT_CONFIG`, `DEBIAN_FRONTEND` with sandbox packages); `docker exec` forwards no host env | `executor.env_passthrough: [NAME, …]` for non-secret host vars (e.g. proxies) |
| **credentials** | Provider / channel keys in gateway env only; never inherited by the sandbox | Subagent CLI backends get only their own `api_key_env`, via a `0600` env file (bwrap) or `docker exec -e NAME` — never in the command text, `script.sh`, `capsule.yaml`, logs, or host argv |
| **persistence** | Workspace persisted; capsules per run | `~/.bobaclaw/runs/<run_id>/` |
| **resource limits** | Executor/backend dependent | Docker image, bwrap namespaces |

## Executor profiles (v1)

| Profile | Backend | Notes |
|---------|---------|-------|
| `bwrap-default` | bubblewrap | `executor.network: false`; no network |
| `bwrap-networked` | bubblewrap | `--share-net`; selected by the default config (`executor.network: true`) |
| `readonly` | bubblewrap | read-only root binds |
| `systemd-run` | systemd-run | Falls back to bwrap |
| `host-danger` | host shell | Explicit approval only; never default |

Every execution:

1. Saves script + `capsule.yaml` before run.
2. Records Run Ledger events.
3. Captures stdout, stderr, exit code, `result.json`.

### Network default (decision)

The config default is `executor.network: true` so package installs and web fetches work
out of the box; changing it would break existing deployments. The fail-closed posture below
is a **recommended operator setting**, not the shipped default. Keys cannot leak through the
environment either way (the sandbox env is cleared), but with network on, anything the
sandbox can read (workspace files) can be exfiltrated.

## Recommended default (untrusted input)

- scoped workspace writes only;
- no host filesystem outside binds;
- no credentials in sandbox (enforced: cleared env);
- no network unless task requires it and policy allows (`executor.network: false`);
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

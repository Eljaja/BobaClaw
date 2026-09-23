# Agent change plan

## Goal

Close the two exploitable security gaps found in the June 2026 review: API keys leaking into the exec sandbox environment, and the unauthenticated HTTP gateway exposed on `0.0.0.0` in the Docker template.

## Context

Priority **P0 (security hotfix)** — first item of the reliability/autonomy review roadmap.

Findings:

- `BwrapExecutor` builds the `bwrap` command without `--clearenv` (`crates/bobaclaw-executor/src/bwrap.rs`), so the parent process environment — including `OPENAI_API_KEY` and `TELEGRAM_BOT_TOKEN` — is inherited into the sandbox. A plain `printenv` from a prompt-injected command exfiltrates keys. This contradicts `harness/sandbox-contract.md` ("keys not injected into sandbox by default").
- Subagent external backends build `export <KEY_ENV>=<key> && <command>` (`crates/bobaclaw-agent/src/subagent/backends/mod.rs`), so keys land in capsule stdout/stderr logs and process listings.
- Gateway endpoints `/v1/chat/completions`, `/api/agent`, `/api/spawn/*` have no authentication and no rate limiting; `docker/config.docker.yaml` binds `0.0.0.0:18790`.
- `ExecutorConfig` defaults to `network: true` and `sandbox_packages: true`, while harness docs describe network-off as the default posture.

## Scope

### In scope

- Add `--clearenv` plus a minimal explicit whitelist (`PATH`, `HOME`, `LANG`, `TERM`, sandbox-package vars already set via `--setenv`) to all bwrap invocations.
- Stop exporting API keys inside the subagent backend command string; pass keys via an env file with `0600` permissions bind-mounted into the sandbox, or via `--setenv` only for the child process (never echoed into logs).
- Bearer-token auth middleware for gateway API routes (`/health` stays open); token from config/env (`gateway.auth_token_env`).
- Change `docker/config.docker.yaml` default bind to `127.0.0.1` and document the explicit opt-in for LAN exposure.
- Align `executor.network` default with the documented fail-closed posture (or update `harness/sandbox-contract.md` to match reality — pick one, no drift).

### Out of scope

- Full credential vault / proxy (nanoClaw OneCLI pattern) — follow-up.
- Rate limiting (tracked in `observability-health-metrics.md` follow-ups).
- Docker executor env hardening beyond key passing (container env is already minimal).

## Files likely to change

- `crates/bobaclaw-executor/src/bwrap.rs`
- `crates/bobaclaw-executor/src/sandbox.rs`
- `crates/bobaclaw-agent/src/subagent/backends/mod.rs`
- `crates/bobaclaw-gateway/src/server.rs` (auth middleware)
- `crates/bobaclaw-core/src/config.rs` (gateway auth config, executor network default)
- `docker/config.docker.yaml`
- `config.example.yaml`
- `harness/sandbox-contract.md` (align contract with implementation)

## Implementation steps

1. Add `--clearenv` + whitelist to `append_base_ro_binds` / bwrap call sites; add a regression test that `printenv OPENAI_API_KEY` inside the sandbox returns empty.
2. Rework subagent backend key injection to env-file or `--setenv`; ensure keys never appear in `script.sh`, capsule logs, or `ps` output.
3. Add gateway bearer auth (axum middleware); reject when token configured and missing/mismatched; keep `/health` unauthenticated.
4. Flip Docker template bind to `127.0.0.1`; update deploy docs for explicit exposure.
5. Resolve the `network: true` default vs documented posture; update `harness/sandbox-contract.md` so contract matches code.
6. Run validation.

## Validation

```bash
make ci
cargo test -p bobaclaw-executor -p bobaclaw-gateway -p bobaclaw-agent
```

Additional checks:

- Manual: `bobaclaw agent --message "run printenv"` shows no API keys.
- Manual: gateway request without bearer token returns 401 when auth is configured.

## Risks

- `--clearenv` may break user commands relying on inherited env (e.g. proxies); mitigate with a small documented whitelist and a config escape hatch.
- Auth middleware breaks existing local clients until they add the token; default remains no-auth unless token configured (documented).

## Rollback plan

Revert the branch. New config fields use serde defaults, so old configs keep working; the Docker template change is config-only.

## Completion notes

Status: implemented on branch `fix/security`. Manual Linux checks (below) still to run on a
real host before relying on this in production.

### Drift from the plan

- **Docker template bind stays `0.0.0.0`** inside the container (a published port cannot
  reach a container-loopback listener). Instead: the gateway fails closed on non-loopback
  binds without a token, and `docker-compose.prod.yml` publishes on `127.0.0.1` only.
- **Network default unchanged** (`executor.network: true`) to avoid breaking deployments;
  `harness/sandbox-contract.md` and ADR 003 now document the real default and recommend
  `network: false` for untrusted input.
- **Auth is not opt-in for exposed binds** (plan said "default remains no-auth unless token
  configured"): no token is allowed only on loopback binds, with a warning.
- **Extra (not in original plan):** `docker-socket-proxy` (tecnativa v0.5.0) replaces the
  direct `/var/run/docker.sock` mount in the gateway container; gateway uses
  `DOCKER_HOST=tcp://docker-socket-proxy:2375` over an `internal: true` network. Enabled:
  PING, VERSION, INFO, CONTAINERS, EXEC, IMAGES, POST. This narrows the API surface but is
  not a root boundary (container create + host binds remain possible).
- **Extra:** `executor.env_passthrough` escape hatch (non-secret vars only), inline
  `gateway.auth_token`, `docker/.env` gitignored, capsule artifacts now written *before* the
  bwrap/docker run (matches the contract "saves script + capsule.yaml before run").

### Changed files

- `crates/bobaclaw-executor/src/{bwrap.rs,backend.rs,docker.rs,lib.rs}` — `--clearenv` +
  whitelist (and `env_clear()` on the bwrap/systemd-run process), `SandboxEnv`, `0600`
  secret env file, `SandboxExecutor::exec_command_with_secrets`, `docker exec -e NAME`.
- `crates/bobaclaw-agent/src/subagent/backends/mod.rs` — no more `export KEY=… && cmd`.
- `crates/bobaclaw-core/src/config.rs` — `gateway.auth_token_env` / `auth_token`,
  `executor.env_passthrough`.
- `crates/bobaclaw-gateway/src/{auth.rs,server.rs,lib.rs}`, `Cargo.toml` — bearer middleware.
- `docker-compose.prod.yml`, `docker/config.docker.yaml`, `docker/.env.example`,
  `scripts/docker-prod-deploy.sh`, `.github/workflows/deploy.yml`, `.gitignore`.
- `config.example.yaml`, `harness/sandbox-contract.md`, `docs/adr/003-executor-profiles.md`,
  `docs/as-built.md`, `README.md`.

### Validation run

- `cargo fmt --all` — clean.
- `cargo clippy --workspace --all-targets` — 26 warnings, identical count to `main`
  (all pre-existing; none introduced).
- `cargo test --workspace --no-fail-fast` — 173 passed, 2 failed (pre-existing macOS-path
  failures in `bobaclaw-executor` `docker_mount::tests`); baseline `main` 156 passed / same 2
  failed (+17 new tests).
- `make check-structure scan-secrets eval-smoke` — OK.
- `docker compose -f docker-compose.prod.yml config` — OK.

### Known gaps / still to verify manually (Linux host)

- `bobaclaw agent --message "run printenv"` (bwrap backend) shows no API keys.
- Codex / Claude Code subagent backends still authenticate via the sourced env file.
- Prod stack through the socket proxy: sandbox `docker exec`, Obscura `docker run -i`,
  entrypoint `docker pull`, `bobaclaw doctor`. HAProxy's 10 min client/server timeout can
  cut idle attached streams (MCP hub reconnects once; long silent `exec` would fail).
- Rate limiting — still out of scope.

### Follow-up work

- Credential vault / proxy so subagent keys never enter the sandbox at all.
- Rootless Docker / authz plugin (or a purpose-built broker) for a real host-root boundary.

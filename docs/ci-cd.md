# CI/CD for BobaClaw (agent-first)

CI is the enforcement layer for agent-first development on this repository.

## Pipeline levels

### 1. Structural gate

`scripts/check_repo_structure.py` verifies harness files: `AGENTS.md`, plan template, eval smoke suite, PR template, CI workflow, harness contracts, Cursor rules.

### 2. Safety gate

`scripts/scan_secrets.py` — lightweight pattern scan (not a replacement for gitleaks).

### 3. Mechanical gate (Rust)

From `Makefile`:

```bash
make fmt-check
make clippy
make test
```

Combined as `make lint`. Agent handoff gate: `make ci` (= harness + tests). Full Rust gate before merge: `make ci-full` or `make lint`.

### 4. Behavioral gate

Smoke eval contract: `make eval-smoke` confirms `evals/smoke/repository-contracts.yaml` and structure checks pass.

Future: model-based regression evals on schedule or before release.

### 5. Release gate

`.github/workflows/deploy.yml` — build and push images on GitHub-hosted runners, then **deploy on a self-hosted runner** in the homelab LAN (`runs-on: self-hosted` by default). Release requires passing tests and operator review for risky capability changes.

## Homelab self-hosted runner

The `deploy` job does not SSH from the cloud. It runs on a [self-hosted runner](https://docs.github.com/en/actions/hosting-your-own-runners) installed on the deploy host (for example `192.168.88.220`).

| Setting | Where | Default |
|---------|-------|---------|
| Runner label | GitHub → Settings → Variables → `SELF_HOSTED_RUNNER_LABEL` | `self-hosted` |
| Deploy directory | GitHub → Settings → Variables → `DEPLOY_PATH` | `/opt/bobaclaw` |

When registering the runner, keep the `self-hosted` label or set `SELF_HOSTED_RUNNER_LABEL` to match your custom labels (for example `homelab`).

The runner host needs:

- outbound HTTPS to `github.com` and `ghcr.io`;
- Docker and `docker compose`;
- a clone of this repo at `DEPLOY_PATH` with `docker/.env` secrets;
- persistent data at `DEPLOY_PATH/data/` (bind-mounted to `/data` in the container: `config.yaml`, `state.db`, `workspace/`).

No inbound SSH from the internet is required.

## Workflows

| Workflow | Trigger | Runner | Purpose |
|----------|---------|--------|---------|
| `ci.yml` | PR, push to `main` | `ubuntu-latest` | Harness structure, secrets, Rust fmt/clippy/test |
| `deploy.yml` | push to `main`, tags | `ubuntu-latest` (build) + **self-hosted** (deploy) | Docker build/push + `scripts/docker-prod-deploy.sh` (gateway, embedded scheduler, telegram polling) |

After deploy, the self-hosted job waits for `/health`, log line `telegram bot connected`, and `scheduler running`. Requires `docker/.env` with `TELEGRAM_BOT_TOKEN` on the server.

## Agent-generated PR checks

For agent-generated PRs (checkbox in PR template):

- plan file linked when applicable;
- validation commands listed;
- risky directory changes include matching harness/eval updates;
- CI green;
- human review before merge.

## Evidence artifacts

Upload eval traces and reports when deeper scenarios exist. Run Ledger capsules under `~/.bobaclaw/runs/` support runtime post-mortems.

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

Existing `.github/workflows/deploy.yml` — build, push images, deploy on `main`. Release requires passing tests and operator review for risky capability changes.

## Workflows

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| `ci.yml` | PR, push to `main` | Harness structure, secrets, Rust fmt/clippy/test |
| `deploy.yml` | push to `main`, tags | Docker build + homelab deploy |

## Agent-generated PR checks

For agent-generated PRs (checkbox in PR template):

- plan file linked when applicable;
- validation commands listed;
- risky directory changes include matching harness/eval updates;
- CI green;
- human review before merge.

## Evidence artifacts

Upload eval traces and reports when deeper scenarios exist. Run Ledger capsules under `~/.bobaclaw/runs/` support runtime post-mortems.

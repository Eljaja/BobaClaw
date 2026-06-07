# Evals

Smoke checks and scenario definitions for agent-first repository behavior on BobaClaw.

## Purpose

Evals complement `cargo test`. They verify the repo remains usable and safe for autonomous engineering workflows and that harness contracts stay intact.

## Smoke suite

`evals/smoke/repository-contracts.yaml` — minimum repository contracts on every PR.

Enforced by:

- `scripts/check_repo_structure.py`
- `make eval-smoke`
- `.github/workflows/ci.yml`

## Good evals check artifacts

Prefer checking:

- required files;
- command output;
- tool contract sections;
- sandbox policy sections;
- PR body fields;
- plan files.

Avoid evals that only inspect plausible prose from a model.

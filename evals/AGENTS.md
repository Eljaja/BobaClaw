# Evals directory instructions

Evals in this directory should be deterministic, cheap enough for CI when possible, and tied to concrete repository or agent behavior.

## Rules

- Write all files in this directory in **English**.
- Prefer artifact-based checks over subjective prose grading.
- Add regression scenarios for real failures under `evals/regression/` (future).
- Keep PR smoke evals fast and dependency-light.
- Do not require external network access for default CI evals.
- Document expected inputs, outputs, and artifacts for every scenario.

## Required validation

```bash
make eval-smoke
make ci
```

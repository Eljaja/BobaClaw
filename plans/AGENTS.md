# Plans directory instructions

Plans are durable task records for agent-generated or high-risk work on BobaClaw.

## Feature backlog

Ordered feature specs (F1–F16) live in [`backlog.md`](backlog.md). Each feature has a dedicated plan under `active/` (e.g. `f1-approval-flow.md`). Implement in backlog order unless a plan notes a dependency override. Move a plan to `completed/` when merged.

## Rules

- Write all files in this directory in **English**.
- Use `plans/templates/agent-change-plan.md` for new plans.
- Keep active plans in `plans/active/` while work is in progress.
- Move completed plans to `plans/completed/` when done.
- A plan must name goal, scope, constraints, changed files, validation, risks, and rollback.
- The final PR summary should match the plan or explicitly explain drift.

## When a plan is required

A plan is required for:

- multi-file changes;
- CI/CD or deploy workflow changes;
- harness / tool / sandbox / policy changes;
- `prompt.rs` or agent loop changes;
- security-sensitive work;
- agent-generated work;
- architecture or ADR changes.

## Validation

After implementation, run `make ci` and record output in completion notes.

# Tools: skills_list, skill_view, skill_manage

Workspace procedural memory (OpenClaw-style skills). Implementation: `crates/bobaclaw-agent/src/tools/skills.rs`, `crates/bobaclaw-skills/`.

## skills_list

### Purpose

List installed workspace skills with enabled/disabled status.

### Input

Empty object `{}`.

### Side effects

Read-only.

---

## skill_view

### Purpose

Read a skill's `SKILL.md` by name.

### Input

```json
{ "name": "string" }
```

### Side effects

Read-only.

---

## skill_manage

### Purpose

Create, update, or delete workspace skills.

### Input

```json
{
  "action": "create|patch|edit|delete|write_file|remove_file",
  "name": "string",
  "content": "string",
  "old_string": "string",
  "new_string": "string",
  "file_path": "string",
  "file_content": "string",
  "category": "string"
}
```

Required: `action`, `name`.

### Side effects

- Writes under `~/.bobaclaw/workspace/<group>/skills/`.
- `delete` / `remove_file` are destructive within skill scope.

### Approval requirements

- Medium risk: workspace-scoped writes acceptable by default.
- Deleting skills another user relies on: operator judgment.

### Failure modes

Validation errors from `SkillManager` / guard — read message, fix args, retry.

### Telemetry

File paths changed; no Run Ledger capsule (not exec-backed).

### Tests

`cargo test -p bobaclaw-skills`, skill forge tests if present.

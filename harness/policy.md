# Agent safety policy (BobaClaw)

Classifies operations for the **runtime agent** and for **repo contributors** (Cursor agents).

## Risk classes

### Low risk

- reading workspace/repo files;
- editing docs in scoped paths;
- running local deterministic checks (`make ci`, `cargo test`);
- `skills_list`, `skill_view`.

Approval: not required.

### Medium risk

- changing CI/CD or harness docs;
- changing `prompt.rs` or tool schemas;
- `skill_manage` create/patch in workspace;
- `schedule` within delay limits;
- `exec` in default sandbox without network;
- adding Rust dependencies.

Approval: usually not required; requires plan and PR notes for multi-file work.

### High risk

- modifying sandbox/executor boundaries;
- `host-danger` profile;
- `exec` with network on production gateway;
- MCP tools with browser/network (Obscura);
- pairing/auth/channel policy changes;
- deploy workflow or release automation;
- deleting persistent state or migrations.

Approval: required (operator for runtime; human review for repo).

### Prohibited by default

- committing secrets;
- exfiltrating user/DM content;
- disabling safety gates without replacement;
- hiding telemetry from reviewers;
- destructive host operations without explicit request and rollback;
- inventing command output not returned by tools.

## Policy implementation layers

| Layer | BobaClaw |
|-------|----------|
| Instructions | `prompt.rs`, workspace `BOBACLAW.md`, repo `AGENTS.md` |
| Runtime | executor profiles, pairing, tool arg validation |
| Approval gates | `host-danger`, channel policies |
| Tests | `cargo test`, `scripts/test-*.sh` |
| Evals | `evals/smoke/`, future regression scenarios |
| CI | `make ci`, `.github/workflows/ci.yml` |
| Human review | PR template, Telegram operator |

## PR requirements for policy changes

Policy changes must include rationale, threat impact, tests/evals, migration notes, rollback plan.

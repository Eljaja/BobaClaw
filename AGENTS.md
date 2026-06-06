# AGENTS.md — repo contributors (Cursor / dev agents)

This file guides humans and IDE agents working **on the BobaClaw codebase**. It is **not** injected into the runtime agent — that uses workspace `BOBACLAW.md` under `~/.bobaclaw/workspace/<group>/`.

## System prompt (`crates/bobaclaw-agent/src/prompt.rs`)

The runtime system prompt is assembled in `build_system_prompt()`. It follows a **stable-tier** layout (Hermes-style) plus workspace bootstrap files (OpenClaw-style): identity, agent loop, tool discipline, compaction semantics, then optional `SOUL.md` / `BOBACLAW.md` / `MEMORY.md` from the user workspace.

### Do not add small implementation details to the prompt

Keep `prompt.rs` focused on **durable agent behavior**, not transient stack facts.

**Belongs in the prompt:** identity, agent loop, tool-use enforcement, task completion, memory/scheduling/skills/MCP *usage* rules, compaction handoff semantics, tone and language.

**Does not belong in the prompt:** executor backends (bubblewrap, Docker, flags), specific config keys that change often, sandbox mount paths, CLI subcommands, version-specific package paths, or other low-level internals. Put those in `docs/`, `config.example.yaml`, workspace `BOBACLAW.md` / `TOOLS.md`, or code — not in the cached system prompt.

When in doubt: if the detail is “how BobaClaw is built” rather than “how the agent should behave,” leave it out of `prompt.rs`.

### Agent loop best practices

The prompt must describe an **advanced agent loop**, not a single-shot Q&A:

1. **Observe → act → verify** — read tool output, decide what is still missing, call more tools until done.
2. **Multi-step autonomy** — chain `exec` / `schedule` / MCP across turns; do not stop after one command when the user asked for a deliverable.
3. **Failure recovery** — on error, diagnose with another tool call and try an alternative; do not invent stdout or exit codes.
4. **Parallelism** — independent reads/checks may be issued together when safe.
5. **Final answer discipline** — user-facing text only when work is complete or blocked; no plans or “I will run…” without a tool call in the same turn.

Runtime enforcement lives in `crates/bobaclaw-agent/src/turn.rs` (iteration limit, action nudges, empty-response retries). Prompt text should stay aligned with that loop, not duplicate magic numbers.

### Editing checklist

- Prefer short, stable English sections (prompt-cache friendly).
- Avoid duplicating the same rule in three constants — merge or cross-reference mentally.
- User- or deployment-specific rules → workspace `BOBACLAW.md`, not `prompt.rs`.
- After changes, run `cargo test -p bobaclaw-agent prompt`.

## Workspace vs repo

| File | Audience |
|------|----------|
| `AGENTS.md` (repo root) | Cursor / contributors editing BobaClaw |
| `workspace-examples/home/BOBACLAW.md` | Template for end-user agent workspace |
| `BOBACLAW.md` in `~/.bobaclaw/workspace/<group>/` | Injected into runtime agent context |

## Related docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — agent loop, executor, gateway
- [docs/adr/003-executor-profiles.md](docs/adr/003-executor-profiles.md) — sandbox profiles (not for `prompt.rs`)

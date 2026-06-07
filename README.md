# BobaClaw

Workspace for designing a personal AI assistant in the **Claw** ecosystem (OpenClaw and related projects).

Runtime is **Rust** in `crates/`; reference snapshots live in `references/` (read-only).

## Quick start

```bash
# WSL / Linux
cd bobaClaw
cargo build --release
export OPENAI_API_KEY=sk-...
./target/release/bobaclaw init
./target/release/bobaclaw doctor
./target/release/bobaclaw chat              # interactive REPL
./target/release/bobaclaw agent --message "Hello"
./target/release/bobaclaw gateway start     # http://127.0.0.1:18790
```

Sandbox capsule smoke test: `bobaclaw agent --message "run: echo hello"`.

### Optional: Obscura browser (MCP)

```bash
make install-obscura-mcp          # pull Obscura image + config snippet
# uncomment obscura in ~/.bobaclaw/config.yaml (Docker stdio; see config.example.yaml)
bobaclaw doctor                   # mcp obscura: OK, 12 tool(s)
```

## Agent-first harness

This repository follows the **harness-engineering** template for agentic development (based on [Harness-engineering](https://github.com/Eljaja/Harness-engineering)):

| Path | Purpose |
|------|---------|
| [AGENTS.md](AGENTS.md) | Operating contract for Cursor / contributors |
| [.cursor/rules/](.cursor/rules/) | Cursor rules for workflow, Rust, prompt, contracts |
| [plans/](plans/) | Plans for multi-file and high-risk changes |
| [harness/](harness/) | Tool, sandbox, and policy contracts |
| [evals/](evals/) | Smoke scenarios for CI |

Before handoff: `make ci`. Before merge to `main`: `make ci-full` (or `make lint`).

**Language:** all contributor instructions (`AGENTS.md`, `.cursor/rules/`, `harness/`, `evals/`, `plans/`, harness docs) are **English only**.

## Documentation

| Document | Contents |
|----------|----------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Target architecture, reference comparison, components, security, phases |
| [docs/agent-first-repository.md](docs/agent-first-repository.md) | Agent-first repository layout |
| [docs/best-practices.md](docs/best-practices.md) | Harness engineering practices |
| [docs/ci-cd.md](docs/ci-cd.md) | CI/CD for agentic development |
| [docs/adr/](docs/adr/) | ADRs: Rust stack, state.db, executor profiles, Skill Forge |

## References

| Project | Path | Role in portfolio |
|---------|------|-------------------|
| OpenClaw | `references/openclaw` | Full feature set, gateway, channels, apps |
| Hermes Agent | `references/hermes-agent` | Learning loop, migration, cloud backends |
| nanoClaw | `references/nanoClaw` | Minimal code, Docker isolation |
| NullClaw | `references/nullclaw` | Edge, Zig, vtable plugins |
| PicoClaw | `references/picoClaw` | Go, lightweight hardware, broad channels |

Update snapshots: `git pull` inside each directory under `references/` (separate git repositories).

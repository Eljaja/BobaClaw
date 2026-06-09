# BobaClaw

**BobaClaw** is a personal AI ChatOps agent built in **Rust**. It executes tasks in an isolated sandbox, exposes HTTP and messaging channels, and runs as a long-lived daemon or single-shot CLI tool.

> Part of the **Claw** ecosystem (OpenClaw and related projects). Runtime code lives in `crates/`.

---

## Prerequisites

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| Rust | 1.75 | latest stable |
| OS | Linux / WSL2 | Linux (Ubuntu 22.04+) |
| Executor backend | bubblewrap | Docker |
| API key | OpenAI-compatible | — |

---

## Quick Start

### Build

```bash
git clone https://github.com/Eljaja/BobaClaw.git
cd BobaClaw
cargo build --release
```

Binary: `./target/release/bobaclaw`

### Configure

```bash
mkdir -p ~/.bobaclaw
cp config.example.yaml ~/.bobaclaw/config.yaml
# Edit config.yaml — at minimum set your API key:
export OPENAI_API_KEY=sk-...
```

### Initialize

```bash
./target/release/bobaclaw init
```

### Check environment

```bash
./target/release/bobaclaw doctor
```

`doctor` validates bubblewrap, Docker, network connectivity, and configuration.

### Interactive chat

```bash
./target/release/bobaclaw chat
```

Readline-powered REPL with terminal Markdown rendering and slash commands.

### One-shot request

```bash
./target/release/bobaclaw agent --message "Hello, what's the current time?"
```

### Telegram bot

```bash
./target/release/bobaclaw channel telegram start
```

Long-polls Telegram Bot API. New users must complete **pairing** before chatting — see [Pairing](#pairing) below.

### HTTP Gateway

```bash
./target/release/bobaclaw gateway start
```

Starts an HTTP server with a REST API and OpenAI-compatible endpoint (`/v1/chat/completions`).

### Scheduler

```bash
# Schedule a one-shot reminder (fires in 5 minutes)
./target/release/bobaclaw schedule add "Check the results" --delay-seconds 300

# List pending tasks
./target/release/bobaclaw schedule list

# Cancel a task
./target/release/bobaclaw schedule cancel <task-id>

# Run the background cron daemon
./target/release/bobaclaw scheduler start
```

### Skills

```bash
# List installed skills
./target/release/bobaclaw skills list

# Show skill contents
./target/release/bobaclaw skills view <skill-name>

# Enable / disable a skill
./target/release/bobaclaw skills enable <skill-name>
./target/release/bobaclaw skills disable <skill-name>

# Generate a skill from a template
./target/release/bobaclaw skill-forge create \
  --name my-skill \
  --description "Does something useful"
```

---

## Architecture

```
BobaClaw
├── crates/
│   ├── bobaclaw              # CLI wrapper (clap)
│   ├── bobaclaw-core         # Config, paths, request model
│   ├── bobaclaw-agent        # Core agent: LLM ↔ tools loop
│   ├── bobaclaw-executor     # Sandbox (bubblewrap / Docker)
│   ├── bobaclaw-gateway      # HTTP API (axum)
│   ├── bobaclaw-channel-telegram  # Telegram Bot API polling
│   ├── bobaclaw-scheduler    # Cron + delayed tasks
│   ├── bobaclaw-skills       # Skill registry and state
│   ├── bobaclaw-skill-forge  # Skill template generator
│   ├── bobaclaw-mcp          # MCP server (Model Context Protocol)
│   ├── bobaclaw-provider     # OpenAI-compatible provider
│   └── bobaclaw-state        # Persistent state storage
├── config.example.yaml       # Annotated configuration file
├── docker-compose.prod.yml   # Production deployment
└── docs/
    ├── ARCHITECTURE.md       # System design and rationale
    ├── features.md           # Feature matrix and backlog
    ├── best-practices.md     # Harness engineering guidelines
    └── ci-cd.md              # CI/CD pipeline reference
```

### Executor sandbox

The agent runs commands in an isolated environment:

| Backend | Isolation | Requirements |
|---------|-----------|-------------|
| `bwrap` | Linux user namespace | `bubblewrap` + user namespaces |
| `docker` | container | Docker daemon running |

Configure in `~/.bobaclaw/config.yaml`:

```yaml
executor:
  backend: docker    # or "bwrap"
  sandbox_packages: true
```

---

## Configuration

Key sections in `~/.bobaclaw/config.yaml`:

```yaml
provider:
  base_url: https://api.openai.com/v1   # or custom OpenAI-compatible endpoint
  api_key_env: OPENAI_API_KEY           # env var holding the key
  model: gpt-4o-mini

default_agent_group: home               # workspace group name

agent:
  max_tool_iterations: 60               # max LLM↔tool loop steps per message
  max_action_retries: 2                 # retries when model doesn't call tools
  max_empty_response_retries: 3         # retries on silent agent responses

context:
  max_history_tokens: 32000
  compact_threshold_tokens: 28000

executor:
  backend: docker                       # "bwrap" or "docker"
  sandbox_packages: true

gateway:
  host: 127.0.0.1
  port: 8080
```

### Environment variables

| Variable | Required | Description |
|----------|----------|-------------|
| `OPENAI_API_KEY` | Yes | API key for the provider |
| `BOBACLAW_HOME` | No | Override `~/.bobaclaw` (default) |
| `BOBACLAW_LOG` | No | Log level: `info`, `debug`, `trace` |

---

## Pairing (Telegram)

New Telegram users must be approved before they can chat with the agent.

1. Add your bot via [@BotFather](https://t.me/BotFather) and save the token.
2. Start a DM with the bot — it will reply with a pairing code.
3. From the host machine:

```bash
# List pending pairing codes
./target/release/bobaclaw pairing list --channel telegram

# Approve a user
./target/release/bobaclaw pairing approve --channel telegram <code>
```

---

## Docker / Production

### Build a Docker image

```dockerfile
# Dockerfile
FROM rust:1.75-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    bubblewrap ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/bobaclaw /usr/local/bin/
ENTRYPOINT ["bobaclaw"]
```

### Run with docker-compose

```bash
cp docker-compose.prod.yml docker-compose.yml
# Set OPENAI_API_KEY and TELEGRAM_BOT_TOKEN in your environment
docker compose up -d
```

### Run without Docker (production)

Use a systemd unit for the gateway and scheduler daemons:

```ini
[Unit]
Description=BobaClaw Gateway
After=network.target

[Service]
ExecStart=/usr/local/bin/bobaclaw gateway start
Restart=always
Environment=OPENAI_API_KEY=<key>
Environment=BOBACLAW_HOME=/var/lib/bobaclaw

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable --now bobaclaw-gateway.service
sudo systemctl enable --now bobaclaw-scheduler.service
```

---

## Contributing

See [AGENTS.md](./AGENTS.md) for the repository operating contract.

### Development workflow

```bash
# 1. Fork and clone
git clone https://github.com/<your-fork>/BobaClaw.git
cd BobaClaw

# 2. Create a branch
git checkout -b feature/my-feature

# 3. Make changes, run tests
cargo test

# 4. Verify structure and secrets scan
python3 scripts/check_repo_structure.py
python3 scripts/scan_secrets.py

# 5. Commit and push
git commit -m "type: concise change description"
git push origin feature/my-feature

# 6. Open a PR against main
```

### Commit convention

```
<type>: <short description>

[optional body]
```

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`.

### CI checklist

- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `python3 scripts/check_repo_structure.py` clean
- [ ] `python3 scripts/scan_secrets.py` clean
- [ ] Smoke eval passes (see `scripts/run_smoke_eval.py`)
- [ ] PR description explains **what** changed and **why**

---

## Documentation

| File | Description |
|------|-------------|
| [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) | System design, rationale, and trade-offs |
| [docs/features.md](./docs/features.md) | Feature matrix and backlog |
| [docs/best-practices.md](./docs/best-practices.md) | Harness engineering guidelines |
| [docs/ci-cd.md](./docs/ci-cd.md) | CI/CD pipeline reference |
| [docs/evals.md](./docs/evals.md) | Evaluation methodology |
| [AGENTS.md](./AGENTS.md) | Repository operating contract for AI agents |

---

## License

MIT — see [Cargo.toml](./Cargo.toml).

# BobaClaw

Рабочая область для проектирования личного AI-ассистента в духе экосистемы **Claw** (OpenClaw и родственные проекты).

Runtime на **Rust** в `crates/`; референсы в `references/` (read-only).

## Быстрый старт

```bash
# WSL / Linux
cd bobaClaw
cargo build --release
export OPENAI_API_KEY=sk-...
./target/release/bobaclaw init
./target/release/bobaclaw doctor
./target/release/bobaclaw chat              # интерактивный REPL
./target/release/bobaclaw agent --message "Привет"
./target/release/bobaclaw gateway start   # http://127.0.0.1:18790
```

Капсула в sandbox: `bobaclaw agent --message "run: echo hello"`.

### Опционально: браузер Obscura (MCP)

```bash
make install-obscura-mcp          # pull образа Obscura + сниппет config
# раскомментируй obscura в ~/.bobaclaw/config.yaml (Docker stdio, см. config.example.yaml)
bobaclaw doctor                   # mcp obscura: OK, 12 tool(s)
```

## Документация

| Документ | Содержание |
|----------|------------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Целевая архитектура BobaClaw, сравнение референсов, компоненты, безопасность, фазы |
| [docs/adr/](docs/adr/) | ADR: Rust stack, state.db, executor profiles, Skill Forge |

## Референсы

| Проект | Путь | Роль в портфеле |
|--------|------|-----------------|
| OpenClaw | `references/openclaw` | Полный feature-set, gateway, каналы, apps |
| Hermes Agent | `references/hermes-agent` | Learning loop, миграция, cloud backends |
| nanoClaw | `references/nanoClaw` | Минимальный код, Docker-изоляция |
| NullClaw | `references/nullclaw` | Edge, Zig, vtable-плагины |
| PicoClaw | `references/picoClaw` | Go, лёгкое железо, широкие каналы |

Обновление снимков: `git pull` внутри каждого каталога в `references/` (отдельные git-репозитории).

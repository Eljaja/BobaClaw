# BobaClaw: возможности и сравнение с эталонами

**Статус:** актуально на 2026-06-05  
**Эталоны:** `references/{openclaw,hermes-agent,nanoClaw,nullclaw,picoClaw}` (read-only снимки)  
**Связанные документы:** [ARCHITECTURE.md](ARCHITECTURE.md), [adr/](adr/)

Этот документ фиксирует, что уже реализовано в BobaClaw MVP, и какие возможности есть у эталонов в `references/`, но ещё отсутствуют в runtime. Используется как backlog и карта заимствований при доработке.

---

## Профили эталонов

| Эталон | Стек | Суть |
|--------|------|------|
| **OpenClaw** | Node/TS | Полный personal AI: gateway, 20+ extensions (discord, slack, whatsapp, browser, voice…), multi-agent, cron, sandbox, companion apps, ClawHub, wizard |
| **Hermes** | Python | Learning loop: 6+ мессенджеров, 40+ tools, память (FTS + плагины), автосоздание skills, `hermes claw migrate`, cloud backends |
| **nanoClaw** | TS + Docker | Минимальный trunk: host + **контейнер на сессию**, SQLite inbox/outbox, pairing, **OneCLI Vault** для ключей |
| **NullClaw** | Zig | Static binary ~678 KB: vtable-плагины, 50+ провайдеров, 19 каналов, 35+ tools, hybrid memory (FTS + vectors), MCP, subagents |
| **PicoClaw** | Go | Edge: gateway + **WebUI**, MCP, model routing, steering/subagents, 15+ каналов, `.security.yml` |

Родословная (см. [ARCHITECTURE.md §3](ARCHITECTURE.md#32-родословная-концептуально)): OpenClaw — корень; от него nanoClaw и Hermes; PicoClaw и NullClaw — параллельные «лёгкие» ветки. BobaClaw задуман как **синтез паттернов**, не форк одного репозитория.

Обновление снимков:

```bash
cd references/<name> && git pull
```

---

## Что уже есть в BobaClaw

**12 Rust crates**, рабочий MVP.

### Архитектура

| Crate | Реализовано |
|-------|-------------|
| `bobaclaw` | CLI: `init`, `doctor`, `agent`, `chat` (REPL, `/compact`, `/new`), `gateway start`, `skills`, `channel telegram start`, `pairing`, `schedule`, `scheduler start` |
| `bobaclaw-core` | `config.yaml`, paths, routing rules, telegram policy, MCP config, cron/scheduler config |
| `bobaclaw-state` | SQLite WAL (`state.db`): sessions, messages, **FTS5 schema**, runs/run_events, pairing, routes, cron, scheduled_tasks, skill_drafts |
| `bobaclaw-provider` | OpenAI-compatible chat + tool loop |
| `bobaclaw-executor` | bubblewrap profiles (`bwrap-default`, networked, readonly); run ledger + capsules |
| `bobaclaw-agent` | Agent loop, compaction (LLM-summarize), tools: `exec`, `schedule`, MCP |
| `bobaclaw-gateway` | axum: `/health`, `/v1/chat/completions`, `/api/agent`; embedded scheduler + optional telegram poll |
| `bobaclaw-channel-telegram` | Long-poll, pairing, group policies, media download, streaming `editMessageText` |
| `bobaclaw-scheduler` | Cron из config + one-shot tasks; embedded или daemon с pidfile |
| `bobaclaw-skills` | `SKILL.md` registry, guard audit |
| `bobaclaw-skill-forge` | `draft-from-run`, `promote` |
| `bobaclaw-mcp` | rmcp child-process hub, prefixed tool names |

### Ключевые возможности MVP

| Область | Реализовано |
|---------|-------------|
| **CLI** | `init`, `doctor`, `agent`, `chat` (REPL, `/compact`, `/new`) |
| **Gateway** | axum: `/health`, `/v1/chat/completions`, `/api/agent` |
| **Каналы** | Только **Telegram** (pairing, groups, streaming edit, медиа) |
| **Sandbox** | bubblewrap (`bwrap-default`); tools: `exec`, `schedule`, `mcp_*` |
| **Автоматизация** | cron в config + one-shot `schedule` + `scheduler start` |
| **Память** | SQLite sessions + **FTS5 schema** + markdown workspace |
| **Skills** | `SKILL.md` registry, guard, Skill Forge (`draft-from-run` → `promote`) |
| **MCP** | rmcp child-process hub |
| **Routing** | YAML `(channel, peer) → agent_group` |

### Заготовки без реализации

| Заготовка | Где | Статус |
|-----------|-----|--------|
| Таблица `approvals` | `migrations/20260603100000_initial.sql` | Схема есть, код approval flow — нет |
| `messages_fts` | та же миграция | FTS5 + триггеры есть, search API/CLI — нет |
| `host-danger` executor | `crates/bobaclaw-executor/src/bwrap.rs` | bail «not implemented» |
| `bobaclaw onboard` | `docs/ARCHITECTURE.md` §6.1 | Только в спецификации |

---

## Пробелы vs эталоны (приоритеты)

### P0 — production remote assistant

| Пробел | Зачем | Эталон | Путь в `references/` |
|--------|-------|--------|----------------------|
| **Второй и последующие каналы** (Discord/Slack/WhatsApp/Signal) | Remote-first без Telegram-only | OpenClaw, PicoClaw | `openclaw/extensions/discord/`, `slack/`, `whatsapp/` · `picoClaw/pkg/channels/` |
| **Gateway как долгоживущий daemon** (systemd, stop/status, hot reload) | Always-on на homelab | OpenClaw, Hermes | `openclaw/src/gateway/` · `hermes-agent/gateway/run.py`, `restart.py` |
| **Multi-model + failover** | Надёжность и cost control | PicoClaw, OpenClaw | `picoClaw/pkg/routing/router.go`, `classifier.go` · OpenClaw model-failover docs |
| **Credential vault / proxy** (ключи вне sandbox агента) | Безопасность tools | nanoClaw | `nanoClaw/src/modules/approvals/onecli-approvals.ts` |
| **Docker / per-session executor** (опция к bwrap) | Сильнее изоляция | nanoClaw | `nanoClaw/docs/architecture.md` · OpenClaw sandbox docs |
| **Steering** (новое сообщение прерывает turn) | UX мессенджера | PicoClaw | `picoClaw/pkg/agent/` · Hermes `gateway/stream_dispatch.py` |
| **Command approval** для опасного exec | Fail-closed security | Hermes, OpenClaw | `hermes-agent/agent/tool_guardrails.py` · BobaClaw: таблица `approvals` |

### P1 — ежедневный UX

| Пробел | Эталон | Путь в `references/` |
|--------|--------|----------------------|
| **Onboard wizard** | OpenClaw, Hermes | `openclaw/src/wizard/setup.ts` · `hermes-agent/agent/onboarding.py` |
| **FTS search** (schema есть, API нет) | Hermes, NullClaw | `hermes-agent/agent/memory_manager.py` · `nullclaw/docs/en/architecture.md` |
| **Web UI / Control panel** | PicoClaw, OpenClaw | `picoClaw/web/frontend/` · `openclaw/apps/` |
| **Модель изоляции каналов** (shared / separate groups) | nanoClaw | `nanoClaw/docs/isolation-model.md`, `src/router.ts` |
| **web_search / web_fetch** | NullClaw, Hermes, PicoClaw | `nullclaw/src/tools/web_search.zig`, `web_fetch.zig` · `picoClaw/pkg/tools/integration/web.go` |
| **File tools** (read/write/edit vs raw exec) | NullClaw, PicoClaw | `nullclaw/src/tools/file_read.zig`, `file_write.zig`, `file_edit.zig` |
| **Migrate from OpenClaw** | Hermes | README + `hermes claw migrate` |
| **Prometheus / observability** | OpenClaw | `openclaw/extensions/diagnostics-prometheus/` |
| **Anthropic native / provider registry** | Hermes, NullClaw | `hermes-agent/providers/` · NullClaw provider vtable |

### P2 — расширяемость

| Пробел | Эталон | Путь в `references/` |
|--------|--------|----------------------|
| **Subagents / delegate / spawn** | Hermes, PicoClaw, NullClaw | `picoClaw/pkg/tools/spawn.go`, `subagent.go` · `nullclaw/src/tools/delegate.zig` |
| **Pluggable channel adapter** (trait / plugin API) | NullClaw, OpenClaw | `nullclaw/src/channels/` · `openclaw/extensions/AGENTS.md` |
| **LLM streaming** (gateway / CLI) | OpenClaw, Hermes | `hermes-agent/gateway/stream_events.py` |
| **Autonomous skill synthesis** | Hermes | `hermes-agent/agent/skill_*.py` · BobaClaw: только manual Skill Forge |
| **Browser automation** | OpenClaw, NullClaw | `openclaw/extensions/browser/` · `nullclaw/src/tools/browser.zig` |
| **Voice / transcription / TTS** | OpenClaw, Hermes | `openclaw/extensions/voice-call/` · `hermes-agent/agent/transcription_provider.py` |
| **Skills marketplace / hub** | OpenClaw, PicoClaw | ClawHub · `picoClaw/web/frontend/src/routes/agent/hub.tsx` |
| **Multi-profile gateways** | Hermes | `hermes-agent/website/docs/user-guide/multi-profile-gateways.md` |
| **Security hardening config** | PicoClaw, OpenClaw | PicoClaw `.security.yml` · OpenClaw exposure runbook |

### P3 — edge / hardware (вне scope v1)

См. [ARCHITECTURE.md §2.2](ARCHITECTURE.md#22-вне-scope-v1).

| Пробел | Эталон | Путь в `references/` |
|--------|--------|----------------------|
| **Native nodes** (iOS/Android/macOS) | OpenClaw | `openclaw/apps/ios/`, `apps/android/`, `apps/macos/` |
| **Ultra-light edge deploy** | NullClaw, PicoClaw | `nullclaw/README.md` · `picoClaw/docs/guides/hardware-compatibility.md` |
| **Hardware / GPIO** | NullClaw, PicoClaw | `nullclaw/src/hardware.zig` · `picoClaw/pkg/tools/hardware/` |
| **Tunnels** (Tailscale, Cloudflare) | NullClaw, OpenClaw | NullClaw tunnel subsystem · OpenClaw Tailscale docs |
| **Vector memory backends** | NullClaw, Hermes | NullClaw memory engines · `hermes-agent/plugins/memory/` |
| **Research / trajectory export** | Hermes | `hermes-agent/agent/trajectory.py` |

---

## Матрица: у кого брать паттерн

| Подсистема | Лучший эталон | Путь |
|------------|---------------|------|
| Ширина каналов | OpenClaw | `references/openclaw/extensions/` |
| Изоляция executor | nanoClaw | `references/nanoClaw/docs/architecture.md` |
| Credential proxy | nanoClaw | `references/nanoClaw/src/modules/approvals/onecli-approvals.ts` |
| Learning / memory loop | Hermes | `references/hermes-agent/agent/memory_manager.py` |
| Migrate с OpenClaw | Hermes | `hermes claw migrate` |
| Edge + model routing | PicoClaw | `references/picoClaw/pkg/routing/` |
| Pluggable architecture | NullClaw | `references/nullclaw/docs/en/architecture.md` |
| Operator UX (wizard, apps) | OpenClaw + PicoClaw | `openclaw/src/wizard/` · `picoClaw/web/frontend/` |

---

## Сводка по интеграциям

| | BobaClaw | OpenClaw | Hermes | nanoClaw | NullClaw | PicoClaw |
|--|:--:|:--:|:--:|:--:|:--:|:--:|
| MCP | ✅ | ✅ | ✅ | — | ✅ | ✅ |
| Telegram | ✅ | ✅ | ✅ | skill | ✅ | ✅ |
| Другие каналы | ❌ | ✅ 20+ | ✅ 6+ | мало | ✅ 19 | ✅ 15+ |
| Sandbox | bwrap | Docker/SSH | cloud | **Docker** | WASM/Docker | container |
| Web UI | ❌ | ✅ apps | dashboard | ❌ | ❌ | ✅ |
| Subagents | ❌ | ✅ | ✅ | ❌ | ✅ | ✅ |
| FTS memory | schema | ✅ | ✅ | SQLite | ✅ hybrid | JSONL |
| Skill auto-gen | manual Forge | ClawHub | **loop** | ❌ | ❌ | hub UI |
| Wizard | ❌ | ✅ | ✅ | ❌ | ❌ | ❌ |

---

## Рекомендуемый порядок доработки

1. **P0:** второй канал (по образцу `bobaclaw-channel-telegram`); systemd unit для gateway+scheduler; secondary model + failover в `bobaclaw-provider`; approval flow поверх таблицы `approvals`.
2. **P1:** `bobaclaw search` на `messages_fts`; minimal Web status (health + sessions) по мотивам PicoClaw `web/`; `web_fetch` tool; `bobaclaw onboard`.
3. **P2:** subagent spawn (PicoClaw pattern); streaming в gateway; optional Docker executor profile.
4. **P3:** только при явной потребности (edge binary, mobile nodes).

---

## Вывод

BobaClaw уже закрывает **ядро Claw DNA**: gateway skeleton, CLI, Telegram+pairing, bwrap exec, SQLite sessions, cron, MCP, skills. Ближе всего к **nanoClaw** по изоляции и к **PicoClaw** по MCP, но без их WebUI и routing.

Главные разрывы с эталонами:

1. **Ширина** — один канал vs 6–20+ у референсов
2. **Операторский UX** — нет wizard, Web UI, systemd daemon
3. **Resilience** — один OpenAI-compatible provider, без failover
4. **Безопасность** — bwrap есть, но нет vault, approvals, Docker-опции
5. **Toolset** — три built-in tools vs web/files/browser/subagents у крупных эталонов

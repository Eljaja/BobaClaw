# BobaClaw

**BobaClaw** — персональный AI-ассистент на базе **Rust**. Работает как ChatOps-агент: читает инструменты, исполняет команды в сандбоксе, подключает внешние каналы (Telegram) и предоставляет HTTP gateway.

> Часть экосистемы **Claw** (OpenClaw и связанные проекты). Разработка ведётся на Rust в `crates/`.

---

## Быстрый старт

### Предварительные требования

| Компонент | Минимум | Рекомендуется |
|-----------|---------|---------------|
| Rust | 1.75 | latest (stable) |
| ОС | Linux / WSL2 | Linux (Ubuntu 22.04+) |
| Executor backend | bubblewrap | Docker |
| API key | OpenAI-compatible | — |

### Установка

```bash
git clone https://github.com/Eljaja/BobaClaw.git
cd BobaClaw
cargo build --release
```

Бинарник появится в `./target/release/bobaclaw`.

### Настройка

```bash
# Скопировать пример конфига
mkdir -p ~/.bobaclaw
cp config.example.yaml ~/.bobaclaw/config.yaml

# Указать API-ключ
export OPENAI_API_KEY=sk-...

# Инициализировать рабочее пространство
./target/release/bobaclaw init
```

### Проверка окружения

```bash
./target/release/bobaclaw doctor
```

`doctor` диагностирует bubblewrap, Docker, сетевые зависимости и конфигурацию.

### Интерактивный чат

```bash
./target/release/bobaclaw chat
```

REPL с поддержкой истории (readline), Markdown-рендерингом в терминале и slash-командами.

### Однострочный запрос

```bash
./target/release/bobaclaw agent --message "Hello, what's the current time?"
```

### Telegram-бот

```bash
./target/release/bobaclaw channel telegram start
```

Долгая polling-загрузка обновлений от Telegram Bot API. Для pairing-кодов см. секцию *Pairing* ниже.

### HTTP Gateway

```bash
./target/release/bobaclaw gateway start
```

Запускает HTTP-сервер с REST API и OpenAI-совместимым эндпоинтом (`/v1/chat/completions`).

### Планировщик задач

```bash
# Добавить отложенную задачу (напоминание через 5 минут)
./target/release/bobaclaw schedule add "Проверить результаты" --delay-seconds 300

# Список задач
./target/release/bobaclaw schedule list

# Отменить задачу
./target/release/bobaclaw schedule cancel <task-id>

# Запустить фоновый cron-процесс
./target/release/bobaclaw scheduler start
```

### Работа с навыками (Skills)

```bash
# Список установленных навыков
./target/release/bobaclaw skills list

# Прочитать содержимое навыка
./target/release/bobaclaw skills view <skill-name>

# Включить / выключить навык
./target/release/bobaclaw skills enable <skill-name>
./target/release/bobaclaw skills disable <skill-name>

# Создать навык из шаблона
./target/release/bobaclaw skill-forge create \
  --name my-skill \
  --description "Does something useful"
```

---

## Архитектура

```
BobaClaw
├── crates/
│   ├── bobaclaw              # CLI-обёртка, main.rs + clap
│   ├── bobaclaw-core         # Конфиг, пути, модель запросов
│   ├── bobaclaw-agent        # Ядро агента: цикл LLM ↔ tools
│   ├── bobaclaw-executor     # Sandbox (bubblewrap / Docker)
│   ├── bobaclaw-gateway      # HTTP API (axum)
│   ├── bobaclaw-channel-telegram  # Telegram Bot API polling
│   ├── bobaclaw-scheduler    # Cron + отложенные задачи
│   ├── bobaclaw-skills       # Skill registry и state
│   ├── bobaclaw-skill-forge  # Генератор шаблонов навыков
│   ├── bobaclaw-mcp          # MCP-сервер (Model Context Protocol)
│   ├── bobaclaw-provider     # OpenAI-совместимый провайдер
│   └── bobaclaw-state        # Персистентное состояние
├── config.example.yaml       # Пример конфигурации
└── docker-compose.prod.yml   # Production-деплой
```

### Песочница (Executor)

Агент исполняет команды в изолированном окружении:

| Backend | Изоляция | Требования |
|---------|----------|------------|
| `bubblewrap` | Linux user namespace | `bubblewrap` + user namespaces |
| `docker` | контейнер | Docker daemon |

Конфигурируется в `config.yaml`:

```yaml
executor:
  backend: docker    # или "bwrap"
  sandbox_packages: true
```

### Skill-навыки

Навыки живут в `~/.bobaclaw/skills/<group>/` и содержат `SKILL.md` с инструкциями для агента. Агент автоматически выбирает релевантные навыки по запросу.

### Subagents

BobaClaw может делегировать подзадачи изолированным суб-агентам. Поддерживаемые бэкенды: `claude-code`, `codex`, `cursor`, `native`. Конфигурируются в `config.yaml` секции `subagents`.

---

## Конфигурация

Основной файл: `~/.bobaclaw/config.yaml` (копия `config.example.yaml`).

### Ключевые секции

```yaml
provider:
  base_url: https://api.openai.com/v1   # OpenAI-совместимый endpoint
  api_key_env: OPENAI_API_KEY            # Переменная окружения с ключом
  model: gpt-4o-mini                     # Модель по умолчанию

agent:
  max_tool_iterations: 60      # Макс. шагов LLM↔tools на сообщение
  max_action_retries: 2
  max_empty_response_retries: 3
  compact_threshold_tokens: 16000   # Порог контекстного сжатия

context:
  max_history_messages: 50
  max_history_chars: 40000

executor:
  backend: docker
  sandbox_packages: true
  bwrap_path: /usr/bin/bwrap

default_agent_group: home
```

### Переменные окружения

| Переменная | Обязательна | Описание |
|------------|:-----------:|----------|
| `OPENAI_API_KEY` | Да | API-ключ провайдера |
| `BOBACLAW_CONFIG` | Нет | Путь к config.yaml (по умолчанию `~/.bobaclaw/config.yaml`) |
| `BOBACLAW_HOME` | Нет | Корневой каталог (по умолчанию `~/.bobaclaw`) |

---

## Docker (production)

```bash
# Сборка production-образа
docker build -t bobaclaw:latest .

# Запуск с docker-compose
docker compose -f docker-compose.prod.yml up -d
```

---

## Pairing (Telegram)

При первом запуске Telegram-канала бот генерирует pairing-код. Пользователь отправляет `/start <code>`, код подтверждается:

```bash
# Посмотреть ожидающие запросы
./target/release/bobaclaw pairing list --channel telegram

# Подтвердить пользователя
./target/release/bobaclaw pairing approve --channel telegram <code>
```

---

## CI / Contributing

```bash
# Линтинг и форматирование
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings

# Тесты
cargo test --all

# Проверка структуры репозитория
python3 scripts/check_repo_structure.py

# Проверка на секреты
python3 scripts/scan_secrets.py
```

См. также [AGENTS.md](AGENTS.md) — операционные правила для агентов и контрибьюторов.

---

## Документация

| Файл | Описание |
|------|----------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Архитектурный обзор и дизайн-решения |
| [docs/features.md](docs/features.md) | Реализованные возможности и бэклог |
| [docs/best-practices.md](docs/best-practices.md) | Harness engineering: безопасная и воспроизводимая работа с агентами |
| [docs/telemetry.md](docs/telemetry.md) | Телеметрия и метрики |
| [docs/evals.md](docs/evals.md) | Оценка и бенчмарки |
| [docs/ci-cd.md](docs/ci-cd.md) | CI/CD пайплайн |
| [AGENTS.md](AGENTS.md) | Контракт работы с репозиторием |

---

## Лицензия

MIT. См. [Cargo.toml](Cargo.toml).

# BobaClaw — common dev commands
#
# Usage:
#   make build          # release binary → target/release/bobaclaw
#   make run            # interactive chat REPL
#   make test           # unit tests
#   make help           # list all targets

CARGO       ?= cargo
BIN         ?= target/release/bobaclaw
BIN_DEBUG   ?= target/debug/bobaclaw
MESSAGE     ?= Hello

COMPOSE_FILE  ?= docker-compose.prod.yml
BOBACLAW_IMAGE ?= bobaclaw/bobaclaw:latest
BOBACLAW_SANDBOX_IMAGE ?= bobaclaw/sandbox:latest
COMPOSE       := docker compose -f $(COMPOSE_FILE)

PYTHON      ?= python3

.PHONY: help build build-dev run test check fmt fmt-check clippy lint clean install \
        init doctor chat agent gateway scheduler sandbox-image sandbox-test \
        install-obscura-mcp stop-obscura-mcp \
        docker-build docker-up docker-down docker-restart docker-logs docker-ps \
        docker-pull docker-doctor docker-env \
        test-integration probe-api check-db check-migrations \
        ci ci-full check-structure scan-secrets eval-smoke

.DEFAULT_GOAL := help

help: ## Show available targets
	@grep -E '^[a-zA-Z0-9_.-]+:.*##' $(MAKEFILE_LIST) | sort | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  \033[36m%-22s\033[0m %s\n", $$1, $$2}'

# --- build ---

build: ## Build release binary (target/release/bobaclaw)
	$(CARGO) build --release

build-dev: ## Build debug binary (faster compile, for local hacking)
	$(CARGO) build

install: build ## Install bobaclaw into ~/.cargo/bin
	$(CARGO) install --path crates/bobaclaw --force

clean: ## Remove build artifacts
	$(CARGO) clean

# --- run ---

run: build chat ## Build release and start interactive chat REPL

chat: build ## Interactive chat REPL (readline, slash commands)
	$(BIN) chat

agent: build ## Single agent turn (MESSAGE="your prompt")
	$(BIN) agent --message "$(MESSAGE)"

gateway: build ## Start HTTP gateway on 127.0.0.1:18790
	$(BIN) gateway start

scheduler: build ## Foreground scheduler daemon (cron + delayed tasks)
	$(BIN) scheduler start

# --- quality ---

test: ## Run all unit tests
	$(CARGO) test --workspace

check: ## Fast compile check without producing binaries
	$(CARGO) check --workspace

fmt: ## Format Rust sources
	$(CARGO) fmt --all

fmt-check: ## Verify formatting (CI-friendly)
	$(CARGO) fmt --all -- --check

clippy: ## Run Clippy lints
	$(CARGO) clippy --workspace --all-targets -- -D warnings

lint: fmt-check clippy test ## fmt + clippy + tests

# --- setup / diagnostics ---

init: build ## Create ~/.bobaclaw config and workspace layout
	$(BIN) init

doctor: build ## Health and environment checks (bwrap, docker, config)
	$(BIN) doctor

# --- sandbox (Docker) ---

sandbox-image: ## Build bobaclaw/sandbox:latest Docker image
	./scripts/build-sandbox-image.sh

sandbox-test: ## Smoke-test Docker sandbox isolation
	./scripts/test-docker-sandbox.sh

# --- MCP (Obscura browser) ---

install-obscura-mcp: ## Pull Obscura image and print Docker stdio MCP config
	./scripts/install-obscura-mcp.sh

stop-obscura-mcp: ## Remove leftover Obscura MCP containers
	docker rm -f bobaclaw-obscura-mcp bobaclaw-mcp-obscura 2>/dev/null || true
	docker rm -f $$(docker ps -aq --filter ancestor=h4ckf0r0day/obscura) 2>/dev/null || true

# --- production Docker (gateway + host Docker for sandbox / Obscura) ---

docker-env: ## Create docker/.env from example if missing
	@test -f docker/.env || (cp docker/.env.example docker/.env && echo "created docker/.env — set OPENAI_API_KEY and TELEGRAM_BOT_TOKEN")

docker-build: ## Build bobaclaw + sandbox images (BOBACLAW_IMAGE=… BOBACLAW_SANDBOX_IMAGE=…)
	BOBACLAW_IMAGE=$(BOBACLAW_IMAGE) BOBACLAW_SANDBOX_IMAGE=$(BOBACLAW_SANDBOX_IMAGE) \
		./scripts/docker-prod-build.sh

docker-up: docker-env ## Start production stack (gateway + scheduler + telegram)
	DEPLOY_PATH=$$(pwd) BOBACLAW_IMAGE=$(BOBACLAW_IMAGE) BOBACLAW_SANDBOX_IMAGE=$(BOBACLAW_SANDBOX_IMAGE) \
		./scripts/docker-prod-deploy.sh

docker-down: ## Stop production stack
	$(COMPOSE) down

docker-restart: ## Restart gateway container
	$(COMPOSE) restart bobaclaw

docker-logs: ## Follow gateway logs
	$(COMPOSE) logs -f bobaclaw

docker-ps: ## Show production stack status
	$(COMPOSE) ps

docker-pull: ## Pull images from registry (set BOBACLAW_IMAGE / BOBACLAW_SANDBOX_IMAGE)
	BOBACLAW_IMAGE=$(BOBACLAW_IMAGE) BOBACLAW_SANDBOX_IMAGE=$(BOBACLAW_SANDBOX_IMAGE) \
		$(COMPOSE) pull

docker-doctor: ## Run bobaclaw doctor inside the gateway container
	$(COMPOSE) exec bobaclaw bobaclaw doctor

# --- integration scripts (require built binary + config) ---



test-integration: build ## Shell integration scripts (exec, net, chat, docker)
	./scripts/test-exec.sh
	./scripts/test-net.sh
	./scripts/test-chat.sh
	./scripts/test-docker-sandbox.sh

probe-api: ## Probe gateway REST API (gateway must be running)
	./scripts/probe-api.sh

check-db: ## Inspect ~/.bobaclaw/state.db schema
	./scripts/check-db.sh

check-migrations: ## List applied SQLx migrations
	./scripts/check-migrations.sh

# --- harness engineering (agent-first CI) ---

ci: check-structure scan-secrets eval-smoke test ## Harness checks + unit tests

ci-full: ci lint ## Harness + fmt + clippy + tests

check-structure: ## Verify required harness files and directories
	$(PYTHON) scripts/check_repo_structure.py

scan-secrets: ## Lightweight secret-pattern scan (stdlib Python)
	$(PYTHON) scripts/scan_secrets.py

eval-smoke: check-structure ## Confirm smoke eval contract is present
	@echo "Smoke eval contract OK: evals/smoke/repository-contracts.yaml"

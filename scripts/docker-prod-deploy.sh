#!/usr/bin/env bash
# Deploy BobaClaw production stack (gateway + embedded scheduler + telegram polling).
#
# Runs from the git checkout (compose + scripts). Persistent data:
#   $DEPLOY_PATH/data/config.yaml  (default DEPLOY_PATH=/opt/bobaclaw)
#
#   DEPLOY_PATH=/opt/bobaclaw ./scripts/docker-prod-deploy.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DEPLOY_PATH="${DEPLOY_PATH:-/opt/bobaclaw}"
DATA_DIR="${BOBACLAW_DATA_DIR:-$DEPLOY_PATH/data}"
COMPOSE_FILE="${COMPOSE_FILE:-$REPO_ROOT/docker-compose.prod.yml}"
CONFIG_FILE="$DATA_DIR/config.yaml"
HEALTH_URL="${HEALTH_URL:-http://127.0.0.1:18790/health}"
HEALTH_TIMEOUT_SECS="${HEALTH_TIMEOUT_SECS:-90}"
LOG_TIMEOUT_SECS="${LOG_TIMEOUT_SECS:-60}"

mkdir -p "$DATA_DIR"
cd "$REPO_ROOT"

if [ -f "$REPO_ROOT/docker/.env" ]; then
  # Optional: BOBACLAW_IMAGE, BOBACLAW_SANDBOX_IMAGE, RUST_LOG, etc.
  # shellcheck disable=SC1091
  set -a
  source "$REPO_ROOT/docker/.env"
  set +a
fi

export BOBACLAW_DATA_DIR="$DATA_DIR"

if [ ! -f "$CONFIG_FILE" ]; then
  echo "note: $CONFIG_FILE not found — first container start will seed a template" >&2
else
  echo "config: $CONFIG_FILE"
fi
echo "repo: $REPO_ROOT"
echo "data: $DATA_DIR"

telegram_enabled_in_config() {
  [ -f "$CONFIG_FILE" ] || return 1
  grep -A20 '^  telegram:' "$CONFIG_FILE" | grep -q 'enabled: true'
}

echo "=== pull images ==="
docker compose -f "$COMPOSE_FILE" pull

echo "=== start gateway (scheduler + telegram are embedded in gateway start) ==="
docker compose -f "$COMPOSE_FILE" up -d --force-recreate --remove-orphans

echo "=== wait for gateway health ==="
deadline=$((SECONDS + HEALTH_TIMEOUT_SECS))
until curl -fsS "$HEALTH_URL" >/dev/null 2>&1; do
  if [ "$SECONDS" -ge "$deadline" ]; then
    echo "error: gateway health check timed out ($HEALTH_URL)" >&2
    docker compose -f "$COMPOSE_FILE" logs --tail=80 bobaclaw || true
    exit 1
  fi
  sleep 2
done
echo "gateway: healthy"

if telegram_enabled_in_config; then
  echo "=== wait for telegram long-poll (from config.yaml) ==="
  deadline=$((SECONDS + LOG_TIMEOUT_SECS))
  until docker compose -f "$COMPOSE_FILE" logs bobaclaw 2>&1 | grep -q "telegram bot connected"; do
    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "error: telegram channel did not start (check bot_token in $CONFIG_FILE)" >&2
      docker compose -f "$COMPOSE_FILE" logs --tail=80 bobaclaw || true
      exit 1
    fi
    sleep 2
  done
  echo "telegram: connected"
else
  echo "note: channels.telegram.enabled is not true in config — skip telegram wait"
fi

echo "=== wait for embedded scheduler ==="
deadline=$((SECONDS + LOG_TIMEOUT_SECS))
until docker compose -f "$COMPOSE_FILE" logs bobaclaw 2>&1 | grep -q "scheduler running"; do
  if [ "$SECONDS" -ge "$deadline" ]; then
    echo "error: embedded scheduler did not start" >&2
    docker compose -f "$COMPOSE_FILE" logs --tail=80 bobaclaw || true
    exit 1
  fi
  sleep 2
done
echo "scheduler: running (embedded)"

docker compose -f "$COMPOSE_FILE" ps
docker compose -f "$COMPOSE_FILE" exec -T bobaclaw bobaclaw doctor || true

echo "=== deploy complete ==="

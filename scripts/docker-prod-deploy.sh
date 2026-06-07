#!/usr/bin/env bash
# Deploy BobaClaw production stack (gateway + embedded scheduler + telegram polling).
#
# Used by GitHub Actions self-hosted runner and manual ops:
#   DEPLOY_PATH=/opt/bobaclaw ./scripts/docker-prod-deploy.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEPLOY_PATH="${DEPLOY_PATH:-/opt/bobaclaw}"
COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.prod.yml}"
HEALTH_URL="${HEALTH_URL:-http://127.0.0.1:18790/health}"
HEALTH_TIMEOUT_SECS="${HEALTH_TIMEOUT_SECS:-90}"
LOG_TIMEOUT_SECS="${LOG_TIMEOUT_SECS:-60}"

cd "$DEPLOY_PATH"
mkdir -p "$DEPLOY_PATH/data"

if [ ! -f docker/.env ]; then
  echo "error: missing $DEPLOY_PATH/docker/.env (OPENAI_API_KEY, TELEGRAM_BOT_TOKEN)" >&2
  exit 1
fi

# shellcheck disable=SC1091
set -a
source docker/.env
set +a

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

if [ -n "${TELEGRAM_BOT_TOKEN:-}" ]; then
  echo "=== wait for telegram long-poll ==="
  deadline=$((SECONDS + LOG_TIMEOUT_SECS))
  until docker compose -f "$COMPOSE_FILE" logs bobaclaw 2>&1 | grep -q "telegram bot connected"; do
    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "error: telegram channel did not start (no 'telegram bot connected' in logs)" >&2
      docker compose -f "$COMPOSE_FILE" logs --tail=80 bobaclaw || true
      exit 1
    fi
    sleep 2
  done
  echo "telegram: connected"
else
  echo "warn: TELEGRAM_BOT_TOKEN empty in docker/.env — telegram channel skipped" >&2
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

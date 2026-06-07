#!/usr/bin/env bash
# One-time server setup for CI/CD deploy (run on the target host as root or via sudo).
#
#   DEPLOY_PATH=/opt/bobaclaw REPO_URL=https://github.com/you/bobaclaw.git ./scripts/docker-server-bootstrap.sh
set -euo pipefail

DEPLOY_PATH="${DEPLOY_PATH:-/opt/bobaclaw}"
REPO_URL="${REPO_URL:-}"

if [ -z "$REPO_URL" ]; then
  echo "error: set REPO_URL to your git remote" >&2
  exit 1
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "error: docker not installed" >&2
  exit 1
fi

mkdir -p "$(dirname "$DEPLOY_PATH")"

if [ -d "$DEPLOY_PATH/.git" ]; then
  echo "repo already exists at $DEPLOY_PATH"
  git -C "$DEPLOY_PATH" pull --ff-only
else
  git clone "$REPO_URL" "$DEPLOY_PATH"
fi

cd "$DEPLOY_PATH"

if [ ! -f docker/.env ]; then
  cp docker/.env.example docker/.env
  echo "created $DEPLOY_PATH/docker/.env — edit secrets before first start"
fi

echo "bootstrap done: $DEPLOY_PATH"
echo "Next on server:"
echo "  1. edit docker/.env (OPENAI_API_KEY, TELEGRAM_BOT_TOKEN)"
echo "  2. set GitHub secrets: DEPLOY_HOST, DEPLOY_USER, DEPLOY_SSH_KEY, DEPLOY_PATH=$DEPLOY_PATH"
echo "  3. push to main → workflow builds images and runs docker compose up -d"

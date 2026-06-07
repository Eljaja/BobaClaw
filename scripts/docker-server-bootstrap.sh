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

mkdir -p "$DEPLOY_PATH/data"
echo "data dir: $DEPLOY_PATH/data (config.yaml, workspace, state.db)"

echo "bootstrap done: $DEPLOY_PATH"
echo "Next on server:"
echo "  1. create/edit $DEPLOY_PATH/data/config.yaml (provider.api_key, channels.telegram.bot_token, …)"
echo "  2. install a self-hosted Actions runner on this host (label: self-hosted)"
echo "  3. optional GitHub repo variables: SELF_HOSTED_RUNNER_LABEL, DEPLOY_PATH=$DEPLOY_PATH"
echo "  4. push to main → workflow builds images in the cloud, deploy job runs here"

#!/usr/bin/env bash
# Pull Obscura and start a long-lived container for BobaClaw MCP (stdio via docker exec).
#
# The published image's HTTP MCP (obscura mcp --http) is not compatible with BobaClaw's
# streamable-HTTP client yet. Instead we keep a running container and attach MCP stdio
# with `docker exec -i … /obscura mcp` from config.yaml.
#
# Usage:
#   ./scripts/install-obscura-mcp.sh
#
# Environment:
#   OBSCURA_MCP_IMAGE=h4ckf0r0day/obscura:latest
#   OBSCURA_MCP_CONTAINER=bobaclaw-obscura-mcp
#   OBSCURA_MCP_PLATFORM=linux/amd64   # auto on arm64 hosts
#   OBSCURA_MCP_CDP_PORT=9222          # optional host port for CDP (Playwright/Puppeteer)
set -euo pipefail

IMAGE="${OBSCURA_MCP_IMAGE:-h4ckf0r0day/obscura:latest}"
CONTAINER="${OBSCURA_MCP_CONTAINER:-bobaclaw-obscura-mcp}"
CDP_PORT="${OBSCURA_MCP_CDP_PORT:-}"

DOCKER_PLATFORM="${OBSCURA_MCP_PLATFORM:-}"
if [[ -z "$DOCKER_PLATFORM" ]]; then
  arch="$(uname -m)"
  if [[ "$arch" == "arm64" || "$arch" == "aarch64" ]]; then
    DOCKER_PLATFORM="linux/amd64"
  fi
fi
PLATFORM_ARGS=()
if [[ -n "$DOCKER_PLATFORM" ]]; then
  PLATFORM_ARGS=(--platform "$DOCKER_PLATFORM")
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "error: docker not found in PATH" >&2
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "error: docker daemon is not running" >&2
  exit 1
fi

if [[ -n "$DOCKER_PLATFORM" ]]; then
  echo "pulling $IMAGE (platform $DOCKER_PLATFORM) ..."
else
  echo "pulling $IMAGE ..."
fi
docker pull "${PLATFORM_ARGS[@]}" "$IMAGE"

if docker ps -a --format '{{.Names}}' | grep -qx "$CONTAINER"; then
  echo "removing existing container $CONTAINER"
  docker rm -f "$CONTAINER" >/dev/null
fi

PORT_ARGS=()
if [[ -n "$CDP_PORT" ]]; then
  PORT_ARGS=(-p "127.0.0.1:${CDP_PORT}:9222")
  echo "starting $CONTAINER (CDP on 127.0.0.1:${CDP_PORT}) ..."
else
  echo "starting $CONTAINER (no host CDP port; MCP only via docker exec) ..."
fi

# Default image CMD: obscura serve --port 9222 --host 0.0.0.0
docker run -d \
  "${PLATFORM_ARGS[@]}" \
  --name "$CONTAINER" \
  --restart unless-stopped \
  "${PORT_ARGS[@]}" \
  "$IMAGE" >/dev/null

echo -n "waiting for container "
ready=0
for _ in $(seq 1 30); do
  if docker ps --filter "name=^${CONTAINER}$" --filter status=running --format '{{.Names}}' | grep -qx "$CONTAINER"; then
    ready=1
    break
  fi
  echo -n "."
  sleep 1
done
echo

if [[ "$ready" != "1" ]]; then
  echo "error: container did not stay up; check: docker logs $CONTAINER" >&2
  exit 1
fi

echo
echo "Obscura is running."
echo "  container: $CONTAINER"
if [[ -n "$CDP_PORT" ]]; then
  echo "  CDP:       ws://127.0.0.1:${CDP_PORT}/devtools/browser"
fi
echo
echo "Add to ~/.bobaclaw/config.yaml:"
cat <<EOF

mcp_servers:
  obscura:
    command: docker
    args:
      - exec
      - -i
      - $CONTAINER
      - /obscura
      - mcp
    enabled: true
    timeout_secs: 180
    connect_timeout_secs: 90

EOF
echo "Then verify: bobaclaw doctor"
echo "Stop: make stop-obscura-mcp"

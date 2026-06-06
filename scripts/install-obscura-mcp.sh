#!/usr/bin/env bash
# Install Obscura MCP for BobaClaw (Docker image + config snippet).
#
# BobaClaw manages a single named container (`bobaclaw-mcp-obscura`) for stdio MCP.
# It stays up for the lifetime of one `bobaclaw chat` / `gateway` process.
# Image is pulled ahead of time here.
#
# Usage:
#   ./scripts/install-obscura-mcp.sh
#   OBSCURA_MCP_STEALTH=1 ./scripts/install-obscura-mcp.sh
#
# Optional HTTP on the host (not Docker): install the Obscura binary and use
# `url: http://127.0.0.1:3000/mcp` in config — see workspace TOOLS.md.
set -euo pipefail

IMAGE="${OBSCURA_MCP_IMAGE:-h4ckf0r0day/obscura}"
CONTAINER="${OBSCURA_MCP_CONTAINER:-bobaclaw-obscura-mcp}"

# Obscura Docker Hub image is amd64-only today; Apple Silicon needs explicit platform.
PLATFORM="${OBSCURA_MCP_PLATFORM:-}"
if [[ -z "$PLATFORM" ]]; then
  case "$(uname -m)" in
    arm64|aarch64) PLATFORM="linux/amd64" ;;
  esac
fi
PLATFORM_ARGS=()
if [[ -n "$PLATFORM" ]]; then
  PLATFORM_ARGS=(--platform "$PLATFORM")
fi

MCP_ARGS=(mcp)
if [[ "${OBSCURA_MCP_STEALTH:-}" == "1" ]]; then
  MCP_ARGS+=(--stealth)
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "error: docker not found in PATH" >&2
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "error: docker daemon is not running" >&2
  exit 1
fi

# Remove legacy detached HTTP container from earlier installs (broken: binds 127.0.0.1 in image).
docker rm -f "$CONTAINER" 2>/dev/null || true

if [[ -n "$PLATFORM" ]]; then
  echo "pulling $IMAGE (platform: $PLATFORM) ..."
else
  echo "pulling $IMAGE ..."
fi
docker pull "${PLATFORM_ARGS[@]}" "$IMAGE"

echo "smoke test (stdio MCP handshake) ..."
if ! timeout 45 docker run --rm "${PLATFORM_ARGS[@]}" -i "$IMAGE" "${MCP_ARGS[@]}" </dev/null >/dev/null 2>&1; then
  echo "warning: smoke test did not finish cleanly (image may still work when BobaClaw attaches stdio)" >&2
fi

DOCKER_ARGS=(run --rm -i)
if [[ -n "$PLATFORM" ]]; then
  DOCKER_ARGS+=(--platform "$PLATFORM")
fi
DOCKER_ARGS+=("$IMAGE")
DOCKER_ARGS+=("${MCP_ARGS[@]}")

cat <<EOF

Obscura MCP image is ready.

  image: $IMAGE
  transport: Docker stdio (one named container per BobaClaw process)
  container: bobaclaw-mcp-obscura (managed automatically; stop extras: make stop-obscura-mcp)

Add to ~/.bobaclaw/config.yaml:

mcp_servers:
  obscura:
    command: docker
    args:
$(printf '      - %s\n' "${DOCKER_ARGS[@]}")
    enabled: true
    timeout_secs: 180
    connect_timeout_secs: 90

Then verify:

  bobaclaw doctor

HTTP long-lived (\`url:\`) works with a native Obscura binary on the host
(\`obscura mcp --http --port 3000\`), not with the current Docker HTTP mode
(the image listens on 127.0.0.1 inside the container).

EOF

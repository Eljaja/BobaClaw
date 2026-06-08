#!/bin/sh
# First-start bootstrap + gateway for production containers.
set -eu

BOBACLAW_HOME="${BOBACLAW_HOME:-/data}"
export BOBACLAW_HOME
# When the gateway runs in Docker, sandbox bind mounts must use the host data dir.
BOBACLAW_HOST_HOME="${BOBACLAW_HOST_HOME:-}"
export BOBACLAW_HOST_HOME

SANDBOX_IMAGE="${BOBACLAW_SANDBOX_IMAGE:-bobaclaw/sandbox:latest}"
OBSCURA_IMAGE="${OBSCURA_MCP_IMAGE:-h4ckf0r0day/obscura}"
PULL_IMAGES="${BOBACLAW_PULL_IMAGES:-1}"

mkdir -p "$BOBACLAW_HOME"

if [ ! -f "$BOBACLAW_HOME/config.yaml" ]; then
  echo "first start: installing docker config template"
  cp /etc/bobaclaw/config.docker.yaml "$BOBACLAW_HOME/config.yaml"
  # shellcheck disable=SC2016
  sed -i "s|bobaclaw/sandbox:latest|${SANDBOX_IMAGE}|g" "$BOBACLAW_HOME/config.yaml"
  bobaclaw init
fi

if [ "$PULL_IMAGES" = "1" ] && command -v docker >/dev/null 2>&1; then
  if docker info >/dev/null 2>&1; then
    echo "pulling sandbox image: $SANDBOX_IMAGE"
    docker pull "$SANDBOX_IMAGE" || echo "warn: sandbox pull failed (will retry on first exec)" >&2
    echo "pulling obscura image: $OBSCURA_IMAGE"
    docker pull "$OBSCURA_IMAGE" || echo "warn: obscura pull failed (will retry on first MCP connect)" >&2
  else
    echo "warn: docker daemon not reachable; skip image pull" >&2
  fi
fi

echo "config: $BOBACLAW_HOME/config.yaml (provider + telegram secrets live here)"
echo "starting bobaclaw gateway (http api + embedded scheduler + telegram long-poll)"
exec bobaclaw gateway start

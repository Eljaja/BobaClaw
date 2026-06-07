#!/usr/bin/env bash
# Build production images: bobaclaw gateway + sandbox.
#
#   ./scripts/docker-prod-build.sh
#   BOBACLAW_IMAGE=ghcr.io/you/bobaclaw:latest BOBACLAW_SANDBOX_IMAGE=ghcr.io/you/bobaclaw-sandbox:latest ./scripts/docker-prod-build.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BOBACLAW_IMAGE="${BOBACLAW_IMAGE:-bobaclaw/bobaclaw:latest}"
BOBACLAW_SANDBOX_IMAGE="${BOBACLAW_SANDBOX_IMAGE:-bobaclaw/sandbox:latest}"

echo "=== building $BOBACLAW_IMAGE ==="
docker build -f "$ROOT/docker/bobaclaw/Dockerfile" -t "$BOBACLAW_IMAGE" "$ROOT"

echo "=== building $BOBACLAW_SANDBOX_IMAGE ==="
BOBACLAW_SANDBOX_IMAGE="$BOBACLAW_SANDBOX_IMAGE" "$ROOT/scripts/build-sandbox-image.sh"

echo "=== done ==="
echo "  gateway: $BOBACLAW_IMAGE"
echo "  sandbox: $BOBACLAW_SANDBOX_IMAGE"
echo "Next: cp docker/.env.example docker/.env && make docker-up"

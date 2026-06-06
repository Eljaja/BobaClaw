#!/usr/bin/env bash
# Build the BobaClaw sandbox image (linux/amd64 + linux/arm64).
#
# Local single-arch (loads into local docker):
#   ./scripts/build-sandbox-image.sh
#
# Multi-arch manifest (requires buildx + registry push):
#   BOBACLAW_SANDBOX_PUSH=1 BOBACLAW_SANDBOX_IMAGE=ghcr.io/you/bobaclaw-sandbox:latest ./scripts/build-sandbox-image.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="${BOBACLAW_SANDBOX_IMAGE:-bobaclaw/sandbox:latest}"
DOCKERFILE="$ROOT/docker/sandbox/Dockerfile"
PLATFORMS="${BOBACLAW_SANDBOX_PLATFORMS:-linux/amd64,linux/arm64}"

if [[ "${BOBACLAW_SANDBOX_PUSH:-}" == "1" ]]; then
  docker buildx build \
    --platform "$PLATFORMS" \
    -f "$DOCKERFILE" \
    -t "$IMAGE" \
    --push \
    "$ROOT"
  echo "pushed $IMAGE ($PLATFORMS)"
  exit 0
fi

# Default: build for the current machine and load locally.
docker build -f "$DOCKERFILE" -t "$IMAGE" "$ROOT"
echo "built $IMAGE (native arch). For multi-arch push: BOBACLAW_SANDBOX_PUSH=1 $0"

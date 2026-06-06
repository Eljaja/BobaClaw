#!/usr/bin/env bash
# Quick check: long-lived Docker sandbox (matches bobaclaw executor.backend=docker).
set -euo pipefail

ROOT="$(mktemp -d)"
trap 'docker rm -f bobaclaw-sandbox-test 2>/dev/null || true; rm -rf "$ROOT"' EXIT

mkdir -p "$ROOT/workspace/home" "$ROOT/runs"
IMAGE="${BOBACLAW_DOCKER_IMAGE:-bobaclaw/sandbox:latest}"
NAME="${BOBACLAW_DOCKER_CONTAINER:-bobaclaw-sandbox-test}"

echo "=== create sandbox container ($IMAGE) ==="
docker rm -f "$NAME" 2>/dev/null || true
docker create \
  --name "$NAME" \
  --label bobaclaw.sandbox=1 \
  --network bridge \
  --cap-drop=ALL \
  --security-opt no-new-privileges \
  --init \
  -v "$ROOT/workspace:/workspace" \
  -v "$ROOT/runs:/runs" \
  "$IMAGE" \
  sleep infinity
docker start "$NAME"

echo "=== exec in workspace ==="
docker exec -w /workspace/home "$NAME" /bin/bash -lc 'echo ok > probe.txt && cat probe.txt'
test -f "$ROOT/workspace/home/probe.txt"

echo "=== network probe (optional) ==="
if docker exec "$NAME" /bin/bash -lc 'command -v curl >/dev/null && curl -fsS --max-time 5 https://example.com >/dev/null'; then
  echo "network: ok"
else
  echo "network: skipped (no curl or no egress)"
fi

echo "=== all checks passed ==="

#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BC=./target/release/bobaclaw
cp -f "$ROOT/config.local.yaml" ~/.bobaclaw/config.yaml 2>/dev/null || true
{
  echo "what can you do"
  sleep 2
  echo "hi"
  echo "/quit"
} | timeout 120 "$BC" chat 2>&1

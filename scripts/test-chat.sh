#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
BC=./target/release/bobaclaw
cp -f config.local.yaml ~/.bobaclaw/config.yaml 2>/dev/null || true
{
  echo "what can you do"
  sleep 2
  echo "hi"
  echo "/quit"
} | timeout 120 "$BC" chat 2>&1

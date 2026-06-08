#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CFG="${BOBACLAW_CONFIG:-$ROOT/config.local.yaml}"

api_key=""
base_url=""
model=""
if [[ -f "$CFG" ]]; then
  api_key=$(grep -E '^[[:space:]]*api_key:' "$CFG" | head -1 | sed 's/.*: //' | tr -d '"' || true)
  base_url=$(grep -E '^[[:space:]]*base_url:' "$CFG" | head -1 | sed 's/.*: //' | tr -d '"' || true)
  model=$(grep -E '^[[:space:]]*model:' "$CFG" | head -1 | sed 's/.*: //' | tr -d '"' || true)
fi

if [[ -z "$api_key" || -z "$base_url" || -z "$model" ]]; then
  echo "probe-api: set BOBACLAW_CONFIG or create config.local.yaml with provider fields" >&2
  exit 1
fi

curl -sS "${base_url}/chat/completions" \
  -H "Authorization: Bearer ${api_key}" \
  -H "Content-Type: application/json" \
  -d "{\"model\":\"${model}\",\"messages\":[{\"role\":\"user\",\"content\":\"hello\"}]}" \
  | python3 -m json.tool | head -40

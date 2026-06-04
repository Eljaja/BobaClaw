#!/usr/bin/env bash
source /mnt/c/Users/ilya/Documents/BobaClaw/bobaClaw/config.local.yaml 2>/dev/null || true
set -a
[ -f /mnt/c/Users/ilya/Documents/BobaClaw/bobaClaw/config.local.yaml ] && {
  api_key=$(grep 'api_key:' /mnt/c/Users/ilya/Documents/BobaClaw/bobaClaw/config.local.yaml | head -1 | sed 's/.*: //')
  base_url=$(grep 'base_url:' /mnt/c/Users/ilya/Documents/BobaClaw/bobaClaw/config.local.yaml | head -1 | sed 's/.*: //')
  model=$(grep 'model:' /mnt/c/Users/ilya/Documents/BobaClaw/bobaClaw/config.local.yaml | head -1 | sed 's/.*: //')
}
set +a
curl -sS "${base_url}/chat/completions" \
  -H "Authorization: Bearer ${api_key}" \
  -H "Content-Type: application/json" \
  -d "{\"model\":\"${model}\",\"messages\":[{\"role\":\"user\",\"content\":\"привет\"}]}" \
  | python3 -m json.tool | head -40

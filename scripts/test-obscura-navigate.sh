#!/usr/bin/env bash
# Quick timing test: obscura MCP browser_navigate → ya.ru
set -euo pipefail

IMAGE="${OBSCURA_MCP_IMAGE:-h4ckf0r0day/obscura}"
TIMEOUT="${1:-120}"

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

cat >"$TMP" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"https://ya.ru"}}}
EOF

echo "navigate https://ya.ru (timeout ${TIMEOUT}s) ..."
START=$(date +%s)
if timeout "$TIMEOUT" docker run --rm -i "$IMAGE" mcp <"$TMP" >"${TMP}.out" 2>"${TMP}.err"; then
  END=$(date +%s)
  echo "OK in $((END - START))s"
  tail -1 "${TMP}.out" | head -c 500
  echo
else
  END=$(date +%s)
  echo "FAILED/TIMEOUT after $((END - START))s"
  tail -3 "${TMP}.err" 2>/dev/null || true
  exit 1
fi

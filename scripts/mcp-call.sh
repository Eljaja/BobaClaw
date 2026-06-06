#!/usr/bin/env bash
# One-shot MCP tools/call with timing. Usage: mcp-call.sh browser_navigate '{"url":"https://ya.ru","waitUntil":"domcontentloaded"}' [timeout_sec]
set -euo pipefail
TOOL="${1:?tool name}"
ARGS="${2:-{}}"
LIMIT="${3:-30}"
IMAGE="${OBSCURA_MCP_IMAGE:-h4ckf0r0day/obscura}"

REQ=$(mktemp)
OUT=$(mktemp)
ERR=$(mktemp)
trap 'rm -f "$REQ" "$OUT" "$ERR"' EXIT

cat >"$REQ" <<EOF
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"$TOOL","arguments":$ARGS}}
EOF

echo "call $TOOL args=$ARGS (limit ${LIMIT}s)"
START=$(date +%s%N)
set +e
timeout --kill-after=5 "$LIMIT" docker run --rm -i "$IMAGE" mcp <"$REQ" >"$OUT" 2>"$ERR"
RC=$?
set -e
END=$(date +%s%N)
MS=$(( (END - START) / 1000000 ))
echo "exit=$RC elapsed=${MS}ms"
if [[ -s "$OUT" ]]; then
  echo "--- stdout ($(wc -l <"$OUT") lines) ---"
  cat "$OUT"
fi
if [[ -s "$ERR" ]]; then
  echo "--- stderr tail ---"
  tail -8 "$ERR"
fi

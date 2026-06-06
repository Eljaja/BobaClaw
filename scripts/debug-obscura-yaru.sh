#!/usr/bin/env bash
# Debug why Obscura MCP hangs on ya.ru
set -euo pipefail

IMAGE="${OBSCURA_MCP_IMAGE:-h4ckf0r0day/obscura}"
WORKDIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== 1. Network from fresh obscura container ==="
docker run --rm "$IMAGE" sh -c 'command -v curl >/dev/null && curl -sS -o /dev/null -w "curl ya.ru: %{http_code} in %{time_total}s\n" --max-time 15 https://ya.ru || echo "no curl in image"' 2>/dev/null || \
  docker run --rm --entrypoint /obscura "$IMAGE" fetch https://ya.ru --dump text --timeout 15 --wait-until domcontentloaded 2>&1 | head -8

echo
echo "=== 2. CLI fetch ya.ru (domcontentloaded, 30s) ==="
START=$(date +%s%3N)
docker run --rm -i "$IMAGE" fetch https://ya.ru --dump text --timeout 30 --wait-until domcontentloaded 2>&1 | grep -v '^2026' | head -15 || true
echo "elapsed: $(($(date +%s%3N) - START))ms"

echo
echo "=== 3. MCP tool schema: browser_navigate ==="
cd "$WORKDIR"
cargo run -q -p bobaclaw-mcp --example bench_obscura 2>/dev/null &
sleep 1
kill %1 2>/dev/null || true

# Direct MCP probe via short rust one-off in bench - use jq on list tools instead
TMP=$(mktemp)
cat >"$TMP" <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"dbg","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
EOF
echo "tools/list (browser_navigate schema):"
timeout 30 docker run --rm -i "$IMAGE" mcp <"$TMP" 2>/dev/null | grep -o '"name":"browser_navigate"[^}]*}' | head -1 || echo "(parse failed, see raw)"
timeout 30 docker run --rm -i "$IMAGE" mcp <"$TMP" 2>/dev/null | python3 -c "
import sys,json
for line in sys.stdin:
    try:
        o=json.loads(line)
        if o.get('id')==2:
            for t in o.get('result',{}).get('tools',[]):
                if t.get('name')=='browser_navigate':
                    import pprint; pprint.pp(t.get('inputSchema',{}))
    except: pass
" 2>/dev/null || true

probe_nav() {
  local label="$1"
  local args="$2"
  local limit="${3:-45}"
  echo
  echo "=== 4.$label MCP browser_navigate ($limit s cap) ==="
  local req
  req=$(mktemp)
  cat >"$req" <<EOF
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"dbg","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"browser_navigate","arguments":$args}}
EOF
  START=$(date +%s%3N)
  if timeout "$limit" docker run --rm -i "$IMAGE" mcp <"$req" >"${req}.out" 2>"${req}.err"; then
    echo "OK in $(($(date +%s%3N) - START))ms"
    tail -1 "${req}.out" | head -c 400; echo
  else
    echo "FAIL/TIMEOUT in $(($(date +%s%3N) - START))ms"
    tail -5 "${req}.err" 2>/dev/null | grep -v '^$' || true
    tail -1 "${req}.out" 2>/dev/null | head -c 400; echo
  fi
  rm -f "$req" "${req}.out" "${req}.err"
}

probe_nav "a default args" '{"url":"https://ya.ru"}' 45
probe_nav "b domcontentloaded" '{"url":"https://ya.ru","waitUntil":"domcontentloaded"}' 45
probe_nav "c load" '{"url":"https://ya.ru","waitUntil":"load"}' 45

rm -f "$TMP"

#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
MCP="$ROOT/mcp-call.sh"

run() {
  echo
  echo "### $1"
  "$MCP" "$2" "$3" "$4"
}

run "example.com default" browser_navigate '{"url":"https://example.com"}' 20
run "ya.ru default" browser_navigate '{"url":"https://ya.ru"}' 30
run "ya.ru domcontentloaded" browser_navigate '{"url":"https://ya.ru","waitUntil":"domcontentloaded"}' 30
run "ya.ru load" browser_navigate '{"url":"https://ya.ru","waitUntil":"load"}' 30

#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export RUST_LOG="${RUST_LOG:-obscura_mcp=debug,bobaclaw_mcp=debug}"

bench() {
  echo
  echo "========== $1 =========="
  shift
  time env "$@" cargo run -q -p bobaclaw-mcp --example bench_obscura 2>&1 | grep -E 'connect|browser_|WARN|ERROR|timed out'
}

docker rm -f $(docker ps -q --filter ancestor=h4ckf0r0day/obscura) 2>/dev/null || true

bench "ya.ru default (180s cap)" MCP_BENCH_URL=https://ya.ru
bench "ya.ru domcontentloaded" MCP_BENCH_URL=https://ya.ru MCP_BENCH_WAIT_UNTIL=domcontentloaded
bench "ya.ru load" MCP_BENCH_URL=https://ya.ru MCP_BENCH_WAIT_UNTIL=load

#!/usr/bin/env bash
cd "$(dirname "$0")/.."
MCP_BENCH_URL=https://ya.ru MCP_BENCH_NAV_TIMEOUT_SECS=30 \
  cargo run -q -p bobaclaw-mcp --example bench_obscura 2>&1 | tail -5

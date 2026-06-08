#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
printf '/new
/quit
' | ./target/release/bobaclaw chat 2>&1

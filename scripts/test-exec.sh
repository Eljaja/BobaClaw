#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BC=./target/release/bobaclaw
echo '=== run: ls -la ==='
"$BC" agent --message 'run: ls -la' 2>&1 | head -15

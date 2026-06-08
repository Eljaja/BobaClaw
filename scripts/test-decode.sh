#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
sqlite3 ~/.bobaclaw/state.db <<'SQL'
INSERT INTO messages (session_id, role, content, timestamp)
VALUES ('sess_298dbbc1-b9af-4c51-9a84-00fe119f331e', x'FFFE', 'x', 1.0);
SQL
printf 'hi\n/quit\n' | ./target/release/bobaclaw chat 2>&1 | tail -15

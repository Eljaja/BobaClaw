#!/usr/bin/env bash
sqlite3 "${HOME}/.bobaclaw/state.db" "PRAGMA table_info(sessions);"
echo "--- try /new SQL ---"
sqlite3 "${HOME}/.bobaclaw/state.db" "UPDATE sessions SET ended_at = 1.0, end_reason = 'test' WHERE id = 'x';" 2>&1 || true

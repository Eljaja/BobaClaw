#!/usr/bin/env bash
sqlite3 "${HOME}/.bobaclaw/state.db" "SELECT * FROM _sqlx_migrations ORDER BY version;"
sqlite3 "${HOME}/.bobaclaw/state.db" "PRAGMA table_info(sessions);" | grep end_reason || echo "no end_reason column"
